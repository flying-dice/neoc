use neoc_vm::errors::{self, NeocError};
use neoc_vm::lua;
use neoc_vm::lua::lib::compile::{TrailerError, detect_embedded_payload_from};
use neoc_vm::lua::std::workers::ShutdownSignal;

use std::io::{self, Write};
use std::path::Path;
use std::{env, fs, process};

use clap::{Parser, Subcommand};

mod cli;

/// neoc — embedded Luau runtime and neoc-language compiler.
///
/// The runtime subcommands (`run`, `emit-headers`) execute and introspect Luau
/// scripts; the compiler subcommands (`build`, `check`, `fmt`, `lsp`) transpile
/// neoc source and serve language tooling.
#[derive(Debug, Parser)]
#[command(name = "neoc", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run one or more Luau scripts (globs expand internally). Pass script
    /// arguments after `--`.
    Run {
        /// Script files or globs (for example `tests/**/*.test.luau`).
        #[arg(required = true)]
        paths: Vec<String>,
        /// Arguments passed through to the script(s) as `_NEOC_ARGS`.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Write every facade-declared module's `.neoc` header under a directory.
    EmitHeaders {
        /// Output directory for the generated headers.
        #[arg(long, value_name = "DIR", default_value = "crates/neoc-vm/src/lua")]
        out: String,
    },
    /// Compiler subcommands: `build`, `check`, `fmt`, `lsp`.
    #[command(flatten)]
    Compiler(cli::Command),
}

fn report_error(err: NeocError) -> ! {
    eprintln!("{:?}", miette::Report::new(err));
    process::exit(1);
}

fn report_script_error(e: mlua::Error, source: &str, path: &str) -> ! {
    if let Some(neoc_err) = errors::extract_require_error(&e, source, path) {
        report_error(neoc_err);
    }
    eprintln!("{e}");
    process::exit(1);
}

// TODO: clean-code - 0.55 - SRP: expand_args mixes glob detection, walking, and process::exit error handling
fn expand_args(args: impl Iterator<Item = String>) -> Vec<String> {
    let mut paths = Vec::new();
    for arg in args {
        if arg.contains('*') || arg.contains('?') || arg.contains('[') {
            let matcher = globset::Glob::new(&arg)
                .unwrap_or_else(|e| {
                    eprintln!("error: invalid pattern {arg}: {e}");
                    process::exit(1);
                })
                .compile_matcher();

            // Walk from the leading literal prefix so we don't scan unrelated trees.
            let base = Path::new(&arg)
                .ancestors()
                .find(|p| !p.to_string_lossy().contains(['*', '?', '[']))
                .and_then(|p| if p == Path::new("") { None } else { Some(p) })
                .unwrap_or(Path::new("."));

            let mut matched = false;
            for entry in walkdir::WalkDir::new(base).sort_by_file_name() {
                let entry = entry.unwrap_or_else(|e| {
                    eprintln!("error: {e}");
                    process::exit(1);
                });
                if entry.file_type().is_file() && matcher.is_match(entry.path()) {
                    paths.push(entry.path().to_string_lossy().into_owned());
                    matched = true;
                }
            }
            if !matched {
                eprintln!("error: no matches for pattern: {arg}");
                process::exit(1);
            }
        } else {
            paths.push(arg);
        }
    }
    paths
}

fn install_shutdown_handler() {
    tokio::spawn(async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            match signal(SignalKind::terminate()) {
                Ok(mut term) => {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {}
                        _ = term.recv() => {}
                    }
                }
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        ShutdownSignal::current().fire();
    });
}

async fn run(paths: &[String], user_args: &[String]) {
    install_shutdown_handler();

    let mut total_passed = 0u32;
    let mut total_failed = 0u32;
    let multi = paths.len() > 1;

    for path in paths {
        let source = fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: {path}: {e}");
            process::exit(1);
        });

        // TODO: clean-code - 0.75 - DRY: engine-init + _NEOC_ARGS binding + scheduler drain duplicated in main() embedded-payload path
        let engine = lua::sandbox::make_engine().unwrap_or_else(|e| {
            report_error(NeocError::EngineInit(e.to_string()));
        });

        if !user_args.is_empty() {
            let tbl = engine.create_table().unwrap();
            for (i, arg) in user_args.iter().enumerate() {
                tbl.set(i + 1, arg.as_str()).unwrap();
            }
            engine.globals().set("_NEOC_ARGS", tbl).unwrap();
        }

        if let Err(e) = engine
            .load(&source)
            .set_name(path.as_str())
            .exec_async()
            .await
        {
            report_script_error(e, &source, path);
        }

        // Drain any tasks spawned by std:task that outlive the main script.
        if let Some(scheduler) = engine.app_data_ref::<lua::std::task::Scheduler>() {
            scheduler.wait_all().await;
            if scheduler.had_task_errors() {
                process::exit(1);
            }
        }

        // TODO: clean-code - 0.6 - DRY: ANSI escape sequences \x1b[32m / \x1b[31m duplicated across pass/fail prints — extract green()/red() helpers
        let is_test = path.ends_with(".test.luau") || path.ends_with(".spec.luau");
        if is_test {
            match lua::lib::test::summary(&engine) {
                Ok((passed, failed)) => {
                    total_passed += passed;
                    total_failed += failed;
                    if multi {
                        if failed == 0 {
                            println!("\x1b[32m✓\x1b[0m {path} ({passed} passed)");
                        } else {
                            println!("\x1b[31m✗\x1b[0m {path} ({passed} passed, {failed} failed)");
                        }
                    } else {
                        println!();
                        if failed == 0 {
                            println!("\x1b[32m{passed} passed\x1b[0m");
                        } else {
                            println!("\x1b[31m{passed} passed, {failed} failed\x1b[0m");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error reading test state: {e}");
                    process::exit(1);
                }
            }
        }
    }

    if multi && (total_passed > 0 || total_failed > 0) {
        println!();
        if total_failed == 0 {
            println!("\x1b[32m{total_passed} passed\x1b[0m");
        } else {
            println!("\x1b[31m{total_passed} passed, {total_failed} failed\x1b[0m");
            process::exit(1);
        }
    } else if total_failed > 0 {
        process::exit(1);
    }
}

/// Check for an embedded payload in the current executable. If found, run it
/// directly and skip the normal filesystem-based entry path.
fn detect_embedded_payload() -> Option<Vec<u8>> {
    let exe = std::env::current_exe().ok()?;
    match detect_embedded_payload_from(&exe) {
        Ok(payload) => payload,
        Err(TrailerError::UnknownVersion(version)) => {
            report_error(NeocError::CorruptPayload {
                detail: format!(
                    "compiled with a newer version of neoc (trailer version {version:#04x}); \
                     please upgrade your neoc installation"
                ),
            });
        }
        Err(TrailerError::CorruptBounds { .. }) => {
            report_error(NeocError::CorruptPayload {
                detail: "embedded payload descriptor is corrupt (offset or length out of range)"
                    .to_string(),
            });
        }
    }
}

/// Writes every facade-declared module's `.neoc` header under `out_dir`, one
/// file per module at `<out_dir>/<namespace>/<module>.neoc` (so `lib:base64`
/// lands at `<out_dir>/lib/base64.neoc`). Prints the path of each file written
/// plus a total.
fn emit_headers(out_dir: &str) {
    let engine = lua::sandbox::make_engine().unwrap_or_else(|e| {
        report_error(NeocError::EngineInit(e.to_string()));
    });
    let headers = lua::modules::headers(&engine).unwrap_or_else(|e| {
        eprintln!("error: collecting headers failed: {e}");
        process::exit(1);
    });

    let base = Path::new(out_dir);
    for (key, header) in &headers {
        let (ns, stem) = key.split_once(':').unwrap_or(("", key.as_str()));
        let dir = base.join(ns);
        fs::create_dir_all(&dir).unwrap_or_else(|e| {
            eprintln!("error: {}: {e}", dir.display());
            process::exit(1);
        });
        let path = dir.join(format!("{stem}.neoc"));
        fs::write(&path, header).unwrap_or_else(|e| {
            eprintln!("error: {}: {e}", path.display());
            process::exit(1);
        });
        println!("{}", path.display());
    }
    println!("\n{} header(s) written to {out_dir}", headers.len());
}

/// Runs the in-memory payload bundled into a self-contained executable.
async fn run_embedded(payload: &[u8]) {
    install_shutdown_handler();

    let user_args: Vec<String> = env::args().skip(1).collect();

    let engine = lua::sandbox::make_engine().unwrap_or_else(|e| {
        report_error(NeocError::EngineInit(e.to_string()));
    });

    if !user_args.is_empty() {
        let tbl = engine.create_table().unwrap();
        for (i, arg) in user_args.iter().enumerate() {
            tbl.set(i + 1, arg.as_str()).unwrap();
        }
        engine.globals().set("_NEOC_ARGS", tbl).unwrap();
    }

    if let Err(e) = engine.load(payload).exec_async().await {
        report_error(NeocError::Runtime(e.to_string()));
    }

    if let Some(scheduler) = engine.app_data_ref::<lua::std::task::Scheduler>() {
        scheduler.wait_all().await;
        if scheduler.had_task_errors() {
            process::exit(1);
        }
    }
}

/// Dispatches a compiler subcommand and exits with its process code.
fn run_compiler(command: cli::Command) -> ! {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    match command.run(&mut err) {
        Ok(0) => process::exit(0),
        Ok(_) => process::exit(1),
        Err(e) => {
            let _ = writeln!(err, "neoc: {e:#}");
            process::exit(1);
        }
    }
}

/// Builds the multi-thread tokio runtime the Luau engine paths drive on, then
/// blocks on `fut`.
fn block_on_runtime<F: std::future::Future<Output = ()>>(fut: F) {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| report_error(NeocError::EngineInit(e.to_string())))
        .block_on(fut);
}

fn main() {
    // A self-contained executable carries its payload in a trailer and is
    // invoked as `<exe> [script args]` — no subcommand. Detect and run it
    // before clap parses, so bundled binaries never see a CLI surface.
    if let Some(payload) = detect_embedded_payload() {
        block_on_runtime(async move { run_embedded(&payload).await });
        return;
    }

    // The compiler subcommands are synchronous; `lsp` builds its own
    // current-thread runtime, so they must not run inside one here. Only the
    // runtime subcommands (`run`, `emit-headers`) drive a tokio runtime.
    match Cli::parse().command {
        Commands::Run { paths, args } => {
            let resolved = expand_args(paths.into_iter());
            if resolved.is_empty() {
                eprintln!("error: no scripts to run");
                process::exit(1);
            }
            block_on_runtime(async move { run(&resolved, &args).await });
        }
        Commands::EmitHeaders { out } => {
            block_on_runtime(async move { emit_headers(&out) });
        }
        Commands::Compiler(command) => run_compiler(command),
    }
}

// ---------------------------------------------------------------------------
// End-to-end test for the Sendable concurrency model.
//
// Spawns the compiled binary against a fixture that uses thread.spawn to
// fan out HTTP workers sharing a listener + counter, hits it with requests,
// and verifies the shared counter aggregates correctly. This is the
// replacement for the old --workers subprocess tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::time::{Duration, Instant};

    static PORT_CTR: AtomicU16 = AtomicU16::new(29000);

    fn next_port() -> u16 {
        PORT_CTR.fetch_add(1, Ordering::Relaxed)
    }

    /// Workspace-level `target/` directory. `CARGO_MANIFEST_DIR` points at the
    /// crate (`crates/neoc`), but Cargo places build output under the workspace
    /// root, so honour `CARGO_TARGET_DIR` when set and otherwise walk up two
    /// levels from the crate manifest to the workspace root.
    fn target_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
            return PathBuf::from(dir);
        }
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/
        p.pop(); // workspace root
        p.push("target");
        p
    }

    fn binary_path() -> PathBuf {
        let mut p = target_dir();
        p.push(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        });
        p.push("neoc");
        p
    }

    fn write_fixture(name: &str, port: u16, script: &str) -> PathBuf {
        let mut dir = target_dir();
        dir.push("spawn-fixtures");
        dir.push(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir fixture");

        let rendered = script.replace("__PORT__", &port.to_string());
        std::fs::write(dir.join("server.luau"), rendered).expect("write server.luau");
        dir
    }

    struct ServerProcess(Option<Child>);

    impl Drop for ServerProcess {
        fn drop(&mut self) {
            if let Some(mut c) = self.0.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    fn spawn_server(fixture: &Path) -> ServerProcess {
        let bin = binary_path();
        assert!(bin.exists(), "binary not built: {}", bin.display());

        let child = Command::new(&bin)
            .arg("run")
            .arg(fixture.join("server.luau"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn neoc");

        ServerProcess(Some(child))
    }

    fn wait_ready(port: u16, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let url = format!("http://127.0.0.1:{port}/__readyz");
        while Instant::now() < deadline {
            if blocking_get(&url).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("server on port {port} never became reachable");
    }

    fn blocking_get(url: &str) -> Result<(u16, String), String> {
        blocking_request("GET", url, None)
    }

    fn blocking_request(
        method: &str,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<(u16, String), String> {
        use std::io::{Read, Write};
        use std::net::TcpStream;

        let url = url.strip_prefix("http://").ok_or("only http://")?;
        let (authority, path) = match url.find('/') {
            Some(i) => (&url[..i], &url[i..]),
            None => (url, "/"),
        };
        let mut stream = TcpStream::connect(authority).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;

        let body_bytes = body.unwrap_or(&[]);
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nContent-Length: {len}\r\n\r\n",
            len = body_bytes.len()
        );
        stream
            .write_all(req.as_bytes())
            .map_err(|e| e.to_string())?;
        if !body_bytes.is_empty() {
            stream.write_all(body_bytes).map_err(|e| e.to_string())?;
        }

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&buf).to_string();

        let mut lines = text.split("\r\n");
        let status_line = lines.next().ok_or("empty response")?;
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .ok_or("no status code")?
            .parse()
            .map_err(|_| "bad status code")?;
        let body_start = text.find("\r\n\r\n").ok_or("no header/body split")? + 4;
        Ok((status, text[body_start..].to_string()))
    }

    fn shutdown(mut server: ServerProcess) {
        let mut child = match server.0.take() {
            Some(c) => c,
            None => return,
        };
        #[cfg(unix)]
        unsafe {
            libc::kill(child.id() as i32, libc::SIGINT);
        }
        #[cfg(not(unix))]
        {
            let _ = child.kill();
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
    }

    #[cfg(unix)]
    mod libc {
        pub const SIGINT: i32 = 2;
        unsafe extern "C" {
            pub fn kill(pid: i32, sig: i32) -> i32;
        }
    }

    /// Multi-worker HTTP server using thread.spawn + shared listener.
    /// Validates the Sendable concurrency model end-to-end.
    #[test]
    fn thread_spawn_multi_worker_http() {
        let port = next_port();
        let fixture = write_fixture(
            "spawn_http",
            port,
            r#"
local thread      = require("std:thread")
local TcpListener = require("std:net").TcpListener
local Counter     = require("std:collections").Counter
local hyper       = require("vnd:hyper")

local listener = TcpListener.bind("127.0.0.1:__PORT__")
local hits     = Counter.new(0)

for i = 1, 4 do
    thread.spawn({
        source = [[
            local hyper  = require("vnd:hyper")
            local shared = (require("std:thread")).shared

            local server = hyper.serve(shared.listener, function(req)
                if req.method == "GET" and req.path == "/__readyz" then
                    return { status = 200, body = "ok" }
                end
                if req.method == "GET" and req.path == "/" then
                    shared.hits:increment()
                    return "OK"
                end
                if req.method == "GET" and req.path == "/hits" then
                    return { status = 200, body = tostring(shared.hits:get()) }
                end
                return { status = 404 }
            end)
        ]],
        shared = { listener = listener, hits = hits },
    })
end

require("std:workers").on_shutdown():wait()
"#,
        );

        let server = spawn_server(&fixture);
        wait_ready(port, Duration::from_secs(10));

        // Hit the server 20 times — requests fan out across 4 workers.
        for _ in 0..20 {
            let (status, body) = blocking_get(&format!("http://127.0.0.1:{port}/")).expect("GET /");
            assert_eq!(status, 200);
            assert_eq!(body, "OK");
        }

        // The shared counter should reflect all 20 hits.
        let (status, body) =
            blocking_get(&format!("http://127.0.0.1:{port}/hits")).expect("GET /hits");
        assert_eq!(status, 200);
        assert_eq!(
            body, "20",
            "shared counter should aggregate across all workers"
        );

        shutdown(server);
    }
}
