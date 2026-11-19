use neoc_vm::errors::{self, NeocError};
use neoc_vm::lua;
use neoc_vm::lua::lib::compile::{TrailerError, detect_embedded_payload_from};
use neoc_vm::lua::std::workers::ShutdownSignal;

use std::path::Path;
use std::{env, fs, process};

use clap::{Parser, Subcommand};

/// neoc — a Lua runtime with a curated standard library. Unsafe Lua globals are
/// stripped and only registered modules load; this is not a security boundary.
#[derive(Debug, Parser)]
#[command(name = "neoc", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run one or more Lua scripts (globs expand internally), or evaluate an
    /// inline snippet with `--eval`. Pass script arguments after `--`.
    Run {
        /// Script files or globs (for example `tests/**/*.test.lua`).
        #[arg(required_unless_present = "eval", conflicts_with = "eval")]
        paths: Vec<String>,
        /// Evaluate the given Lua source string in-process instead of reading
        /// files.
        #[arg(short = 'e', long = "eval", value_name = "SOURCE")]
        eval: Option<String>,
        /// Arguments passed through to the script(s) as `_NEOC_ARGS`.
        #[arg(last = true)]
        args: Vec<String>,
    },
}

fn report_error(err: NeocError) -> ! {
    eprintln!("{:?}", miette::Report::new(err));
    process::exit(1);
}

/// Reports a runtime fault from a script. A `require` fault takes the dedicated
/// module-not-found path first.
fn report_script_error(e: &mlua::Error, source: &str, path: &str) -> ! {
    if let Some(neoc_err) = errors::extract_require_error(e, source, path) {
        report_error(neoc_err);
    }
    eprintln!("{e}");
    process::exit(1);
}

/// Expand one glob pattern to the matching file paths, walking from its leading
/// literal prefix so unrelated trees aren't scanned. Returns an error string
/// (bad pattern, walk failure, or no matches) rather than exiting — the caller
/// owns the `process::exit` policy.
fn glob_expand(pattern: &str) -> Result<Vec<String>, String> {
    let matcher = globset::Glob::new(pattern)
        .map_err(|e| format!("invalid pattern {pattern}: {e}"))?
        .compile_matcher();

    let base = Path::new(pattern)
        .ancestors()
        .find(|p| !p.to_string_lossy().contains(['*', '?', '[']))
        .filter(|&p| p != Path::new(""))
        .unwrap_or(Path::new("."));

    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(base).sort_by_file_name() {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().is_file() && matcher.is_match(entry.path()) {
            paths.push(entry.path().to_string_lossy().into_owned());
        }
    }
    if paths.is_empty() {
        return Err(format!("no matches for pattern: {pattern}"));
    }
    Ok(paths)
}

fn expand_args(args: impl Iterator<Item = String>) -> Vec<String> {
    let mut paths = Vec::new();
    for arg in args {
        if arg.contains('*') || arg.contains('?') || arg.contains('[') {
            match glob_expand(&arg) {
                Ok(matched) => paths.extend(matched),
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
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

/// Builds the Lua engine (unsafe globals stripped, registered modules only),
/// binding `user_args` as the `_NEOC_ARGS` global when any were supplied. Exits
/// the process on engine-init failure.
fn engine_with_args(user_args: &[String]) -> mlua::Lua {
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
    engine
}

/// Drains any tasks spawned via `std:task` that outlive the main script. Exits
/// with code 1 if any of them errored.
async fn drain_scheduler(engine: &mlua::Lua) {
    if let Some(scheduler) = engine.app_data_ref::<lua::std::task::Scheduler>() {
        scheduler.wait_all().await;
        if scheduler.had_task_errors() {
            process::exit(1);
        }
    }
}

/// Wrap `s` in the ANSI green SGR codes used for passing test output.
fn green(s: &str) -> String {
    format!("\x1b[32m{s}\x1b[0m")
}

/// Wrap `s` in the ANSI red SGR codes used for failing test output.
fn red(s: &str) -> String {
    format!("\x1b[31m{s}\x1b[0m")
}

/// Refuses `.neoc` paths before anything runs. A `.neoc` file is the retired
/// language's source: say so plainly rather than feeding it to the Lua loader,
/// and do it up front so `neoc run a.lua old.neoc` runs nothing.
fn reject_neoc_paths(paths: &[String]) {
    for path in paths {
        if Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("neoc"))
        {
            eprintln!(
                "error: {path}: `neoc run` runs Lua (`.lua`) scripts only; \
                 `.neoc` source is no longer supported"
            );
            process::exit(1);
        }
    }
}

async fn run(paths: &[String], user_args: &[String]) {
    reject_neoc_paths(paths);
    install_shutdown_handler();

    let mut total_passed = 0u32;
    let mut total_failed = 0u32;
    let multi = paths.len() > 1;

    for path in paths {
        let source = fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: {path}: {e}");
            process::exit(1);
        });

        let engine = engine_with_args(user_args);

        if let Err(e) = lua::sandbox::load_source(&engine, &source)
            .set_name(path.as_str())
            .exec_async()
            .await
        {
            report_script_error(&e, &source, path);
        }

        drain_scheduler(&engine).await;

        let is_test = path.ends_with(".test.lua") || path.ends_with(".spec.lua");
        if is_test {
            match lua::lib::test::summary(&engine) {
                Ok((passed, failed)) => {
                    total_passed += passed;
                    total_failed += failed;
                    if multi {
                        if failed == 0 {
                            println!("{} {path} ({passed} passed)", green("✓"));
                        } else {
                            println!("{} {path} ({passed} passed, {failed} failed)", red("✗"));
                        }
                    } else {
                        println!();
                        if failed == 0 {
                            println!("{}", green(&format!("{passed} passed")));
                        } else {
                            println!("{}", red(&format!("{passed} passed, {failed} failed")));
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
            println!("{}", green(&format!("{total_passed} passed")));
        } else {
            println!(
                "{}",
                red(&format!("{total_passed} passed, {total_failed} failed"))
            );
            process::exit(1);
        }
    } else if total_failed > 0 {
        process::exit(1);
    }
}

/// Evaluates an inline Lua snippet in-process, with no file on disk, under the
/// chunk name `<eval>`. Script arguments after `--` are bound as `_NEOC_ARGS`.
async fn run_eval(source: &str, user_args: &[String]) {
    install_shutdown_handler();

    let engine = engine_with_args(user_args);

    if let Err(e) = lua::sandbox::load_source(&engine, source)
        .set_name("<eval>")
        .exec_async()
        .await
    {
        report_script_error(&e, source, "<eval>");
    }

    drain_scheduler(&engine).await;
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

/// Runs the in-memory payload bundled into a self-contained executable.
async fn run_embedded(payload: &[u8]) {
    install_shutdown_handler();

    let user_args: Vec<String> = env::args().skip(1).collect();

    let engine = engine_with_args(&user_args);

    if let Err(e) = lua::sandbox::load_source(&engine, payload)
        .exec_async()
        .await
    {
        report_error(NeocError::Runtime(e.to_string()));
    }

    drain_scheduler(&engine).await;
}

/// Builds the multi-thread tokio runtime the Lua engine paths drive on, then
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

    match Cli::parse().command {
        Commands::Run { paths, eval, args } => {
            if let Some(source) = eval {
                block_on_runtime(async move { run_eval(&source, &args).await });
            } else {
                let resolved = expand_args(paths.into_iter());
                if resolved.is_empty() {
                    eprintln!("error: no scripts to run");
                    process::exit(1);
                }
                block_on_runtime(async move { run(&resolved, &args).await });
            }
        }
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
        std::fs::write(dir.join("server.lua"), rendered).expect("write server.lua");
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
            .arg(fixture.join("server.lua"))
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
