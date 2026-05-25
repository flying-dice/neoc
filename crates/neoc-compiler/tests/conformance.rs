//! Data-driven dynamic conformance suite for `lang/CONFORMANCE/dynamic/`.
//!
//! Each `<name>.neoc` case is the executable behavioural contract described in
//! `lang/CONFORMANCE/README.md`; its sibling `<name>.stdout` is the golden
//! output, hand-reasoned from the program's meaning (see `lang/DYNAMIC.md`), not
//! copied from compiler output. Every case ships a sibling `<name>.rs`: the
//! closest Rust translation, the reference the case is anchored to.
//!
//! This file uses `libtest-mimic` (`harness = false`) to emit one visible test
//! per case × target so a failure names exactly which program and backend
//! diverged instead of one opaque blob. Two kinds of trial per target:
//!
//! - `<case>::<target>::source` — the live emit equals the committed, readable
//!   `<case>.lua` / `<case>.js` checked into the corpus. This pins the
//!   transformation itself (codegen changes show up as a diff) and needs no
//!   toolchain, so it runs everywhere. Regenerate the files with
//!   `neoc build lang/CONFORMANCE/dynamic/*.neoc --target <lua|js>`.
//! - `<case>::<target>` — transpile (or, for Rust, compile the `.rs` anchor),
//!   run under the target's interpreter, and assert trailing-newline-trimmed
//!   stdout equals the golden. Marked ignored when the toolchain (`node` /
//!   `lua` / `rustc`) is absent, so the suite still passes without it.
//!
//! Two extra trials guard the corpus shape: it is non-empty, and every `.neoc`
//! has a sibling `.rs`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use libtest_mimic::{Arguments, Failed, Trial};
use neoc_compiler::{Target, transpile};

/// A conformance backend: how to turn a case into a runnable program and run it.
struct TargetSpec {
    /// Short name shown after `::` in the test id (`lua`, `js`, `rust`).
    name: &'static str,
    /// Whether the toolchain this target needs is present on PATH. When false,
    /// the target's run trials are marked ignored rather than failed.
    available: bool,
    /// Produce the program text and run it under the toolchain, returning stdout.
    /// `neoc` is the `.neoc` source, `case` the file stem (for error messages).
    run: fn(neoc: &str, case: &str, neoc_path: &Path) -> Result<String, Failed>,
    /// The compiler [`Target`] and committed-output extension this backend emits
    /// (e.g. `(Target::Lua, "lua")`). When set, a `<case>::<target>::source`
    /// trial pins the emitted code against the readable `<case>.<ext>` file
    /// checked into the corpus. `None` for the Rust anchor, whose `.rs` is
    /// hand-written rather than emitted. This check needs no toolchain.
    emit: Option<(Target, &'static str)>,
}

/// Assert the live emit for `target` equals the committed `<case>.<ext>` golden
/// — the readable output file checked into the corpus.
fn check_source_golden(
    neoc: &str,
    neoc_path: &Path,
    target: Target,
    ext: &str,
    case: &str,
) -> Result<(), Failed> {
    let emitted = emit(neoc, target, case)?;
    let golden_path = neoc_path.with_extension(ext);
    let committed = std::fs::read_to_string(&golden_path).map_err(|e| {
        Failed::from(format!(
            "case `{case}`: read {}: {e}",
            golden_path.display()
        ))
    })?;
    if emitted == committed {
        Ok(())
    } else {
        Err(Failed::from(format!(
            "case `{case}`: emitted .{ext} differs from the committed golden {} — \
             regenerate with `neoc build lang/CONFORMANCE/dynamic/*.neoc --target {target}`\n\
             --- emitted ---\n{emitted}",
            golden_path.display()
        )))
    }
}

/// The first Lua interpreter found on PATH, or `None` if none is present.
fn lua_binary() -> Option<&'static str> {
    ["lua", "lua5.4", "lua5.3", "lua5.1", "luajit"]
        .into_iter()
        .find(|bin| Command::new(bin).arg("-v").output().is_ok())
}

/// Whether the named binary answers `--version` (i.e. is on PATH).
fn binary_present(bin: &str) -> bool {
    Command::new(bin).arg("--version").output().is_ok()
}

/// Write `contents` to a uniquely named temp file with the given extension and
/// return its path. The name combines the process id with a process-wide atomic
/// counter so parallel cases never collide.
fn tempfile(contents: &str, ext: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("neoc_conf_{}_{n}.{ext}", std::process::id()));
    let mut file = std::fs::File::create(&path).expect("create temp file");
    file.write_all(contents.as_bytes())
        .expect("write temp file");
    file.flush().expect("flush temp file");
    path
}

/// A unique temp path (no file created) for a compiled binary.
fn temp_binary_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("neoc_conf_bin_{}_{n}", std::process::id()));
    path
}

/// Transpile `neoc` to `target`, returning the emitted source. Fails the trial
/// with the diagnostics if compilation is not clean.
fn emit(neoc: &str, target: Target, case: &str) -> Result<String, Failed> {
    let out = transpile(neoc, target);
    if out.ok() {
        Ok(out.output)
    } else {
        Err(Failed::from(format!(
            "case `{case}`: unexpected diagnostics: {:#?}",
            out.diagnostics
        )))
    }
}

/// Run `program` (written to a temp `ext` file) under `cmd`, returning stdout.
/// Fails the trial with stderr and the emitted source on a non-zero exit.
/// `extra_args` precede the temp path on the command line — the JS backend uses
/// it to pass `--input-type=…` flags; pass an empty slice otherwise.
fn run_program(
    cmd: &str,
    program: &str,
    ext: &str,
    case: &str,
    kind: &str,
    extra_args: &[&str],
) -> Result<String, Failed> {
    let path = tempfile(program, ext);
    let output = Command::new(cmd)
        .args(extra_args)
        .arg(&path)
        .output()
        .map_err(|e| Failed::from(format!("case `{case}`: spawn `{cmd}` failed: {e}")))?;
    let _ = std::fs::remove_file(&path);
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Failed::from(format!(
            "case `{case}`: {cmd} exited non-zero:\n{}\n--- emitted {kind} ---\n{program}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

/// lua: alias `println` to the built-in `print`, then call `main()`.
fn run_lua(neoc: &str, case: &str, _neoc_path: &Path) -> Result<String, Failed> {
    let lua = lua_binary().expect("lua available (trial would be ignored otherwise)");
    let output = emit(neoc, Target::Lua, case)?;
    let program = format!("println = print\n{output}\nmain()\n");
    run_program(lua, &program, "lua", case, "lua", &[])
}

/// js: route `print` / `println` through `console.log`, then call `main()`. The
/// JS backend emits an ES module (`import`/`export`), so the program runs as one
/// — written to a `.mjs` temp file, which `node` parses as ESM regardless of any
/// surrounding `package.json`.
fn run_js(neoc: &str, case: &str, _neoc_path: &Path) -> Result<String, Failed> {
    let output = emit(neoc, Target::Js, case)?;
    let harness =
        "globalThis.print = (x) => console.log(x);\nglobalThis.println = globalThis.print;\n";
    let program = format!("{harness}{output}\nmain();\n");
    run_program("node", &program, "mjs", case, "js", &[])
}

/// rust: compile the case's `.rs` anchor with `rustc --edition 2021` and run it.
fn run_rust(_neoc: &str, case: &str, neoc_path: &Path) -> Result<String, Failed> {
    let rs_path = neoc_path.with_extension("rs");
    let src = std::fs::read_to_string(&rs_path)
        .map_err(|e| Failed::from(format!("case `{case}`: read {}: {e}", rs_path.display())))?;

    // Copy into a temp file so rustc's crate name and the binary land in the
    // temp dir, not the corpus.
    let src_path = tempfile(&src, "rs");
    let bin_path = temp_binary_path();
    let compile = Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg(&src_path)
        .arg("-o")
        .arg(&bin_path)
        .output()
        .map_err(|e| Failed::from(format!("case `{case}`: spawn rustc failed: {e}")));
    let _ = std::fs::remove_file(&src_path);
    let compile = compile?;
    if !compile.status.success() {
        let _ = std::fs::remove_file(&bin_path);
        return Err(Failed::from(format!(
            "case `{case}`: rustc failed:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        )));
    }

    let output = Command::new(&bin_path)
        .output()
        .map_err(|e| Failed::from(format!("case `{case}`: run anchor binary failed: {e}")));
    let _ = std::fs::remove_file(&bin_path);
    let output = output?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Failed::from(format!(
            "case `{case}`: anchor binary exited non-zero:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

/// The `lang/CONFORMANCE/dynamic/` directory, resolved from the crate manifest.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("lang")
        .join("CONFORMANCE")
        .join("dynamic")
}

/// Every `.neoc` file in the corpus, sorted for a stable iteration order.
fn cases(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read corpus dir")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "neoc"))
        .collect();
    paths.sort();
    paths
}

/// Build the per-case × target trials plus the corpus-shape guards.
fn collect_trials() -> Vec<Trial> {
    let dir = corpus_dir();
    let cases = cases(&dir);

    let targets = [
        TargetSpec {
            name: "lua",
            available: lua_binary().is_some(),
            run: run_lua,
            emit: Some((Target::Lua, "lua")),
        },
        TargetSpec {
            name: "js",
            available: binary_present("node"),
            run: run_js,
            emit: Some((Target::Js, "js")),
        },
        TargetSpec {
            name: "rust",
            available: binary_present("rustc"),
            run: run_rust,
            emit: None,
        },
    ];

    let mut trials = Vec::new();

    // Guard: the corpus must be non-empty.
    let n_cases = cases.len();
    let dir_display = dir.display().to_string();
    trials.push(Trial::test("corpus::non_empty", move || {
        if n_cases == 0 {
            Err(Failed::from(format!(
                "no .neoc cases found under {dir_display}"
            )))
        } else {
            Ok(())
        }
    }));

    // Guard: every `.neoc` has a sibling `.rs` so the two halves can't drift.
    let case_paths = cases.clone();
    trials.push(Trial::test("corpus::every_neoc_has_rs_anchor", move || {
        for neoc in &case_paths {
            let rs = neoc.with_extension("rs");
            if !rs.exists() {
                let case = neoc.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                return Err(Failed::from(format!(
                    "case `{case}`: missing sibling Rust anchor {}",
                    rs.display()
                )));
            }
        }
        Ok(())
    }));

    for neoc in cases {
        let case = neoc
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("case file stem")
            .to_owned();

        for target in &targets {
            // Source golden: the live emit equals the committed readable output
            // file. Needs only the compiler, so it runs even without `node` etc.
            if let Some((emit_target, ext)) = target.emit {
                let neoc_path = neoc.clone();
                let case = case.clone();
                trials.push(Trial::test(
                    format!("{case}::{}::source", target.name),
                    move || {
                        let src = std::fs::read_to_string(&neoc_path)
                            .map_err(|e| Failed::from(format!("case `{case}`: read .neoc: {e}")))?;
                        check_source_golden(&src, &neoc_path, emit_target, ext, &case)
                    },
                ));
            }

            // Run: transpile, run under the toolchain, assert stdout. Ignored
            // when the toolchain is absent.
            let run = target.run;
            let neoc_path = neoc.clone();
            let case = case.clone();
            let trial = Trial::test(format!("{case}::{}", target.name), move || {
                let src = std::fs::read_to_string(&neoc_path)
                    .map_err(|e| Failed::from(format!("case `{case}`: read .neoc: {e}")))?;
                let stdout = run(&src, &case, &neoc_path)?;
                let expected = std::fs::read_to_string(neoc_path.with_extension("stdout"))
                    .map_err(|e| Failed::from(format!("case `{case}`: read .stdout: {e}")))?;
                let got = stdout.trim_end_matches('\n');
                let want = expected.trim_end_matches('\n');
                if got == want {
                    Ok(())
                } else {
                    Err(Failed::from(format!(
                        "case `{case}`: stdout mismatch\n--- expected ---\n{want}\n--- got ---\n{got}"
                    )))
                }
            })
            .with_ignored_flag(!target.available);
            trials.push(trial);
        }
    }

    trials
}

fn main() {
    let args = Arguments::from_args();
    libtest_mimic::run(&args, collect_trials()).exit();
}
