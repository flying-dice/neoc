use neoc_compiler::SourceMap;
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
    /// Run one or more Luau scripts (globs expand internally), or evaluate an
    /// inline snippet with `--eval`. Pass script arguments after `--`.
    Run {
        /// Script files or globs (for example `tests/**/*.test.luau`).
        #[arg(required_unless_present = "eval", conflicts_with = "eval")]
        paths: Vec<String>,
        /// Evaluate the given source string in-process instead of reading
        /// files. Treated as neoc by default (one module; its `main` runs if it
        /// defines one); pass `--lua` to run it as raw Lua/Luau instead.
        #[arg(short = 'e', long = "eval", value_name = "SOURCE")]
        eval: Option<String>,
        /// Treat the `--eval` source as raw Lua/Luau, loaded into the engine
        /// verbatim with no neoc compilation.
        #[arg(long, requires = "eval", conflicts_with_all = ["neoc", "paths"])]
        lua: bool,
        /// Treat the `--eval` source as neoc. This is the default; the flag is
        /// here to state it explicitly.
        #[arg(long, requires = "eval", conflicts_with = "paths")]
        neoc: bool,
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

/// Reports a runtime fault from a script. When `map` is present (a `.neoc`
/// entry run via `neoc run`), every `<path>:<luaLine>` reference in the error
/// and its traceback is rewritten to the originating `.neoc` line, so the fault
/// points at neoc source rather than emitted-Lua lines. A `require` fault still
/// takes the dedicated module-not-found path first.
fn report_script_error_mapped(
    e: mlua::Error,
    source: &str,
    path: &str,
    map: Option<&SourceMap>,
) -> ! {
    if let Some(neoc_err) = errors::extract_require_error(&e, source, path) {
        report_error(neoc_err);
    }
    let text = format!("{e}");
    let rendered = match map {
        Some(map) => map_lua_lines_to_neoc(&text, path, map),
        None => text,
    };
    eprintln!("{rendered}");
    process::exit(1);
}

/// Rewrites every Luau chunk reference for `chunk` in `text` from its emitted-
/// Lua line to the originating neoc line via `map`.
///
/// Luau renders a chunk frame as `[string "<chunk>"]:<line>` (in both the
/// leading `runtime error:` line and each `stack traceback:` frame). Only frames
/// whose chunk name equals `chunk` are rewritten — `[C]` frames and any other
/// chunk are left untouched. On an exact miss the map returns the nearest
/// preceding mapped line, so a line in appended boilerplate still resolves.
fn map_lua_lines_to_neoc(text: &str, chunk: &str, map: &SourceMap) -> String {
    let marker = format!("[string \"{chunk}\"]:");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(&marker) {
        let (before, after) = rest.split_at(pos + marker.len());
        out.push_str(before);
        // Consume the run of digits following the marker — the Lua line number.
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            rest = after;
            continue;
        }
        let lua_line: u32 = digits.parse().unwrap_or(0);
        match map.neoc_line(lua_line) {
            Some(neoc_line) => out.push_str(&neoc_line.to_string()),
            None => out.push_str(&digits),
        }
        rest = &after[digits.len()..];
    }
    out.push_str(rest);
    out
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

// TODO: clean-code - 0.55 - DRY: expand_args (globset+walkdir, exits on no-match) and cli.rs::resolve (glob, warns-and-continues) are two glob expanders for the same job on two dep stacks; unify on one crate with a behaviour flag.
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

/// Builds the sandboxed Luau engine, binding `user_args` as the `_NEOC_ARGS`
/// global when any were supplied. Exits the process on engine-init failure.
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

async fn run(paths: &[String], user_args: &[String]) {
    install_shutdown_handler();

    let mut total_passed = 0u32;
    let mut total_failed = 0u32;
    let multi = paths.len() > 1;

    for path in paths {
        // A `.neoc` entry is transpiled to Lua in-process and run directly, so
        // `neoc run app.neoc` behaves like running a Luau script. `.lua`/`.luau`
        // scripts load verbatim. For a `.neoc` entry the source map travels with
        // the Lua so a runtime fault is reported at the neoc line, not the
        // emitted-Lua line.
        let is_neoc = Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("neoc"));
        let (source, source_map) = if is_neoc {
            match cli::transpile_entry_to_lua(Path::new(path), &mut io::stderr().lock()) {
                Ok(Some(entry)) => (entry.lua, Some(entry.source_map)),
                Ok(None) => process::exit(1),
                Err(e) => {
                    eprintln!("error: {e:#}");
                    process::exit(1);
                }
            }
        } else {
            let source = fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("error: {path}: {e}");
                process::exit(1);
            });
            (source, None)
        };

        let engine = engine_with_args(user_args);

        if let Err(e) = lua::sandbox::load_source(&engine, &source)
            .set_name(path.as_str())
            .exec_async()
            .await
        {
            report_script_error_mapped(e, &source, path, source_map.as_ref());
        }

        drain_scheduler(&engine).await;

        let is_test = path.ends_with(".test.luau") || path.ends_with(".spec.luau");
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

/// Evaluates an inline snippet in-process, with no file on disk, under the chunk
/// name `<eval>`. By default the snippet is neoc: transpiled to Lua first, with
/// its source map carried along so a runtime fault is reported at the neoc line.
/// With `as_lua` set, the snippet is raw Lua/Luau loaded into the engine
/// verbatim — no compilation, no source map. Script arguments after `--` are
/// bound as `_NEOC_ARGS`.
async fn run_eval(source: &str, as_lua: bool, user_args: &[String]) {
    install_shutdown_handler();

    let (script, source_map) = if as_lua {
        (source.to_string(), None)
    } else {
        match cli::transpile_source_to_lua(source, &mut io::stderr().lock()) {
            Ok(Some(entry)) => (entry.lua, Some(entry.source_map)),
            Ok(None) => process::exit(1),
            Err(e) => {
                eprintln!("error: {e:#}");
                process::exit(1);
            }
        }
    };

    let engine = engine_with_args(user_args);

    if let Err(e) = lua::sandbox::load_source(&engine, &script)
        .set_name("<eval>")
        .exec_async()
        .await
    {
        report_script_error_mapped(e, &script, "<eval>", source_map.as_ref());
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

    let engine = engine_with_args(&user_args);

    if let Err(e) = lua::sandbox::load_source(&engine, payload)
        .exec_async()
        .await
    {
        report_error(NeocError::Runtime(e.to_string()));
    }

    drain_scheduler(&engine).await;
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
        Commands::Run {
            paths,
            eval,
            lua,
            neoc: _,
            args,
        } => {
            if let Some(source) = eval {
                block_on_runtime(async move { run_eval(&source, lua, &args).await });
            } else {
                let resolved = expand_args(paths.into_iter());
                if resolved.is_empty() {
                    eprintln!("error: no scripts to run");
                    process::exit(1);
                }
                block_on_runtime(async move { run(&resolved, &args).await });
            }
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

    // ---- source map: `neoc run` runtime-fault line rewriting ----------------
    //
    // `map_lua_lines_to_neoc` is the consumer side of the #56 source map: it
    // rewrites every `[string "<chunk>"]:<luaLine>` reference in a Luau runtime
    // fault to its originating neoc line. The map's own tests cover lookup
    // (`SourceMap::neoc_line`); these pin the two consumer behaviours those
    // don't reach — the string rewrite itself (chunk-selective), and the
    // unmapped-line fallback that leaves a line verbatim.
    use neoc_compiler::{SourceMap, Target, transpile};

    #[test]
    fn map_lua_lines_rewrites_matching_chunk_frames_to_neoc_lines() {
        // A `struct` forces the Luau backend to prepend an inline `type`
        // preamble, shifting the body down — so an emitted line's number differs
        // from its neoc origin, which is what makes the rewrite observable.
        let src = "struct Point { x: number }\npub fn read(p: Point) -> number {\n  p.x\n}\n";
        let out = transpile(src, Target::Luau);
        assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
        let map = out.source_map;
        assert!(!map.is_empty(), "a Luau transpile must carry a map");

        // First emitted line whose origin differs from its own number — the
        // preamble shift guarantees one exists.
        let (lua_line, neoc_line) = (1..=200)
            .find_map(|l| map.neoc_line(l).filter(|n| *n != l).map(|n| (l, n)))
            .expect("a shifted, mapped emitted line");

        let chunk = "app.neoc";
        let err = format!(
            "runtime error: [string \"{chunk}\"]:{lua_line}: boom\n\
             stack traceback:\n\
             \t[string \"{chunk}\"]:{lua_line}: in function 'read'\n\
             \t[C]: in ?\n\
             \t[string \"other.luau\"]:{lua_line}: in main chunk"
        );
        let got = super::map_lua_lines_to_neoc(&err, chunk, &map);

        // Every matching-chunk frame now carries the neoc line; the emitted line
        // is gone.
        assert!(
            got.contains(&format!("[string \"{chunk}\"]:{neoc_line}:")),
            "matching frames should carry neoc line {neoc_line}:\n{got}"
        );
        assert!(
            !got.contains(&format!("[string \"{chunk}\"]:{lua_line}:")),
            "emitted line {lua_line} should be rewritten away:\n{got}"
        );
        // A `[C]` frame and a different chunk are left untouched.
        assert!(got.contains("[C]: in ?"), "[C] frame untouched:\n{got}");
        assert!(
            got.contains(&format!("[string \"other.luau\"]:{lua_line}:")),
            "a non-matching chunk must not be rewritten:\n{got}"
        );
    }

    #[test]
    fn map_lua_lines_leaves_unmapped_lines_verbatim() {
        // An empty map — a module lowered without source text — knows no
        // correspondence, so every line must survive verbatim: not
        // dropped, not zeroed. This is the `None` fallback arm of the rewrite.
        let chunk = "app.neoc";
        let err = format!(
            "runtime error: [string \"{chunk}\"]:5: boom\n\
             stack traceback:\n\t[string \"{chunk}\"]:5: in main chunk"
        );
        let got = super::map_lua_lines_to_neoc(&err, chunk, &SourceMap::default());
        assert_eq!(got, err, "an empty map passes every line through unchanged");
    }

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
