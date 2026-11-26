mod errors;

use errors::NeocError;
use neoc_vm::modules::lib::compile::{TrailerError, detect_embedded_payload_from};
use neoc_vm::modules::std::workers::workers::ShutdownSignal;

use std::path::Path;
use std::{env, fs};

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
    neoc_vm::exit(1);
}

/// Reports a runtime fault from a script. A `require` fault takes the dedicated
/// module-not-found path first.
fn report_script_error(e: &mlua::Error, source: &str, path: &str) -> ! {
    if let Some(neoc_err) = errors::extract_require_error(e, source, path) {
        report_error(neoc_err);
    }
    eprintln!("{e}");
    neoc_vm::exit(1);
}

/// Expand one glob pattern to the matching file paths, walking from its leading
/// literal prefix so unrelated trees aren't scanned. Returns an error string
/// (bad pattern, walk failure, or no matches) rather than exiting — the caller
/// owns the exit policy.
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
                    neoc_vm::exit(1);
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
    let engine = neoc_vm::make_engine().unwrap_or_else(|e| {
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
    if let Some(scheduler) = engine.app_data_ref::<neoc_vm::scheduler::Scheduler>() {
        scheduler.wait_all();
        if scheduler.had_task_errors() {
            neoc_vm::exit(1);
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
            neoc_vm::exit(1);
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
            neoc_vm::exit(1);
        });

        let engine = engine_with_args(user_args);

        if let Err(e) = neoc_vm::load_source(&engine, &source)
            .set_name(path.as_str())
            .exec_async()
            .await
        {
            report_script_error(&e, &source, path);
        }

        drain_scheduler(&engine).await;

        let is_test = path.ends_with(".test.lua") || path.ends_with(".spec.lua");
        if is_test {
            let (passed, failed) = neoc_vm::modules::lib::test::summary(&engine);
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
            neoc_vm::exit(1);
        }
    } else if total_failed > 0 {
        neoc_vm::exit(1);
    }
}

/// Evaluates an inline Lua snippet in-process, with no file on disk, under the
/// chunk name `<eval>`. Script arguments after `--` are bound as `_NEOC_ARGS`.
async fn run_eval(source: &str, user_args: &[String]) {
    install_shutdown_handler();

    let engine = engine_with_args(user_args);

    if let Err(e) = neoc_vm::load_source(&engine, source)
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

    if let Err(e) = neoc_vm::load_source(&engine, payload).exec_async().await {
        report_error(NeocError::Runtime(e.to_string()));
    }

    drain_scheduler(&engine).await;
}

/// Builds the multi-thread tokio runtime the Lua engine paths drive on, then
/// blocks on `fut`.
///
/// The workers have `VM_THREAD_STACK` of stack, and `fut` runs on a worker (not on the main thread, whose stack the
/// operating system decides), so every thread that polls the VM is known to have it.
fn block_on_runtime<F: std::future::Future<Output = ()> + Send + 'static>(fut: F) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(neoc_vm::VM_THREAD_STACK)
        .build()
        .unwrap_or_else(|e| report_error(NeocError::EngineInit(e.to_string())));
    runtime.block_on(async move {
        if let Err(e) = tokio::spawn(fut).await {
            match e.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(e) => report_error(NeocError::Runtime(e.to_string())),
            }
        }
    });
    // an engine that outlives the run (a worker thread still has it) must not leave its processes behind
    neoc_vm::kill_processes();
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
                    neoc_vm::exit(1);
                }
                block_on_runtime(async move { run(&resolved, &args).await });
            }
        }
    }
}
