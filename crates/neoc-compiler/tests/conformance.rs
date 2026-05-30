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
//!   `<case>.ts` / `<case>.luau` / `<case>.py` checked into the corpus. This
//!   pins the transformation itself (codegen changes show up as a diff) and
//!   needs no toolchain, so it runs everywhere. Regenerate the files with
//!   `neoc build lang/CONFORMANCE/dynamic/*.neoc --target <ts|luau|py>`. The
//!   three targets pin the runnable module text only; the `.pyi` typings sidecar
//!   is covered by the codegen crates' own tests. (`lua`/`js` are not selectable
//!   targets — `luau`/`ts` exercise the same lowering and downgrade to them.)
//! - `<case>::<target>` — transpile (or, for Rust, compile the `.rs` anchor),
//!   run under the target's interpreter, and assert trailing-newline-trimmed
//!   stdout equals the golden. Marked ignored when the toolchain (`bun` /
//!   `rustc` / `luau` / `python3` >= 3.10) is absent, so the suite still passes
//!   without it.
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
    /// Short name shown after `::` in the test id (`luau`, `ts`, `rust`).
    name: &'static str,
    /// Whether the toolchain this target needs is present on PATH. When false,
    /// the target's run trials are marked ignored rather than failed.
    available: bool,
    /// Produce the program text and run it under the toolchain, returning stdout.
    /// `neoc` is the `.neoc` source, `case` the file stem (for error messages).
    run: fn(neoc: &str, case: &str, neoc_path: &Path) -> Result<String, Failed>,
    /// The compiler [`Target`] and committed-output extension this backend emits
    /// (e.g. `(Target::Luau, "luau")`). When set, a `<case>::<target>::source`
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
fn run_program(
    cmd: &str,
    program: &str,
    ext: &str,
    case: &str,
    kind: &str,
) -> Result<String, Failed> {
    let path = tempfile(program, ext);
    let output = Command::new(cmd)
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

/// luau: the self-typed Luau module is self-contained — inlined runtime and
/// inline `Result`/`Option` type aliases, no `neoc-lib`. Alias `println` to the
/// built-in `print`, append `main()`, and run under the `luau` CLI.
fn run_luau(neoc: &str, case: &str, _neoc_path: &Path) -> Result<String, Failed> {
    let output = emit(neoc, Target::Luau, case)?;
    let program = format!("println = print\n{output}\nmain()\n");
    run_program("luau", &program, "luau", case, "luau")
}

/// ts: the JS runtime body with neoc types woven on, run under `bun` — which
/// resolves extensionless relative imports and strips types. A module that uses
/// `Result`/`Option` `import`s the shared runtime from `./neoc-lib`, so the
/// harness writes `neoc-lib.js` into the temp directory beside the `.ts` (as
/// `run_js` does) before running. `print` / `println` route through
/// `console.log`.
fn run_ts(neoc: &str, case: &str, _neoc_path: &Path) -> Result<String, Failed> {
    let output = emit(neoc, Target::Ts, case)?;
    let lib_path = std::env::temp_dir().join("neoc-lib.js");
    std::fs::write(&lib_path, neoc_codegen_js::neoc_lib_js())
        .map_err(|e| Failed::from(format!("case `{case}`: write neoc-lib.js: {e}")))?;
    let harness =
        "globalThis.print = (x) => console.log(x);\nglobalThis.println = globalThis.print;\n";
    let program = format!("{harness}{output}\nmain();\n");
    run_program("bun", &program, "ts", case, "ts")
}

/// Find a Python interpreter that is at least 3.10 (needed for `match`/`case`),
/// returning its command name; `None` when none is on PATH.
fn python_3_10() -> Option<&'static str> {
    [
        "python3.14", "python3.13", "python3.12", "python3.11", "python3.10", "python3", "python",
    ]
    .into_iter()
    .find(|cand| {
        let Ok(out) = Command::new(cand)
            .args([
                "-c",
                "import sys; print(sys.version_info[0], sys.version_info[1])",
            ])
            .output()
        else {
            return false;
        };
        if !out.status.success() {
            return false;
        }
        let v = String::from_utf8_lossy(&out.stdout);
        let mut it = v.split_whitespace();
        matches!((it.next(), it.next()), (Some("3"), Some(min)) if min.parse::<u32>().is_ok_and(|m| m >= 10))
    })
}

/// python: the module `import`s its runtime from `.neoc_lib`, so it must run as
/// a package member. Build a unique temp package (`neoc_lib.py`, `__init__.py`,
/// the module as `case.py`), inject `print`/`println` into `builtins` as a
/// neoc-style formatter so the program's output calls resolve and a `bool`
/// renders `true`/`false` (matching the target-independent `.stdout` golden, as
/// Lua and JS do natively) rather than Python's `True`/`False`. Then import
/// `main` and run it under a Python >= 3.10 interpreter.
fn run_python(neoc: &str, case: &str, _neoc_path: &Path) -> Result<String, Failed> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let python = python_3_10().expect("python >= 3.10 (trial ignored otherwise)");
    let output = emit(neoc, Target::Python, case)?;

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("neoc_conf_py_{}_{n}", std::process::id()));
    let pkg = dir.join("genpkg");
    std::fs::create_dir_all(&pkg)
        .map_err(|e| Failed::from(format!("case `{case}`: create temp package: {e}")))?;
    let write = |path: PathBuf, body: &[u8]| -> Result<(), Failed> {
        std::fs::write(&path, body)
            .map_err(|e| Failed::from(format!("case `{case}`: write {}: {e}", path.display())))
    };
    write(pkg.join("__init__.py"), b"")?;
    write(
        pkg.join("neoc_lib.py"),
        neoc_codegen_python::neoc_lib_py().as_bytes(),
    )?;
    write(pkg.join("case.py"), output.as_bytes())?;
    let driver = "import builtins\n\
         _real_print = print\n\
         def _fmt(x):\n    \
             return ('true' if x else 'false') if isinstance(x, bool) else str(x)\n\
         def _neoc_print(x):\n    \
             _real_print(_fmt(x))\n\
         builtins.print = _neoc_print\n\
         builtins.println = _neoc_print\n\
         from genpkg.case import main\n\
         main()\n";
    write(dir.join("driver.py"), driver.as_bytes())?;

    let result = Command::new(python)
        .arg("driver.py")
        .current_dir(&dir)
        .output()
        .map_err(|e| Failed::from(format!("case `{case}`: spawn `{python}` failed: {e}")));
    let _ = std::fs::remove_dir_all(&dir);
    let result = result?;
    if result.status.success() {
        Ok(String::from_utf8_lossy(&result.stdout).into_owned())
    } else {
        Err(Failed::from(format!(
            "case `{case}`: {python} exited non-zero:\n{}\n--- emitted py ---\n{output}",
            String::from_utf8_lossy(&result.stderr)
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
/// The conformance backends: the `rust` anchor plus the three selectable targets
/// (`ts`, `luau`, `python`). `lua`/`js` are not selectable — `luau`/`ts` exercise
/// the same lowering and downgrade to them. Each `::source` golden pin needs no
/// toolchain; each run trial is ignored when its toolchain is absent.
fn target_specs() -> [TargetSpec; 4] {
    [
        TargetSpec {
            name: "rust",
            available: binary_present("rustc"),
            run: run_rust,
            emit: None,
        },
        TargetSpec {
            name: "ts",
            available: binary_present("bun"),
            run: run_ts,
            emit: Some((Target::Ts, "ts")),
        },
        TargetSpec {
            name: "luau",
            available: binary_present("luau"),
            run: run_luau,
            emit: Some((Target::Luau, "luau")),
        },
        TargetSpec {
            name: "python",
            available: python_3_10().is_some(),
            run: run_python,
            emit: Some((Target::Python, "py")),
        },
    ]
}

fn collect_trials() -> Vec<Trial> {
    let dir = corpus_dir();
    let cases = cases(&dir);

    let targets = target_specs();

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
