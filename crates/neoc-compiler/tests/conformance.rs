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
//!   `<case>.lua` checked into the corpus. This pins the transformation itself
//!   (codegen changes show up as a diff) and needs no toolchain, so it runs
//!   everywhere. Regenerate the files by running this suite with
//!   `NEOC_BLESS=1`, which rewrites each golden from the live emit.
//! - `<case>::<target>` — transpile (or, for Rust, compile the `.rs` anchor),
//!   run, and assert trailing-newline-trimmed stdout equals the golden. Lua
//!   runs in-process on `LuaJIT` (`tests/common`), so it always runs. Only the
//!   Rust anchor depends on a toolchain (`rustc`); it is marked ignored when
//!   `rustc` is absent.
//!
//! A case is either a single file (`<name>.neoc`) or a directory
//! (`<name>/main.neoc` plus the sibling modules it `use`s). A directory case
//! compiles through the project model ([`transpile_project`]), pins one readable
//! golden per module, and runs with the non-entry modules served to `require`.
//!
//! A directory case cannot carry `#[test]` fns: each module's `__tests` table is
//! local to its chunk, out of the walk's reach, so the run trial rejects such a
//! case rather than silently dropping its tests.
//!
//! A case may carry a `<name>.fails` marker (`<name>/main.fails` for a directory
//! case): the sad-path contract. The program must compile clean, then raise at
//! run time an error whose message contains the marker's text; its `.stdout`
//! pins what it printed before the raise. The Rust anchor does not apply (a
//! rejection has no Rust analogue) and is neither required nor run.
//!
//! Eight `harness::*` self-test trials pin the harness's own sad-path and guard
//! logic: each feeds an input the harness must refuse and passes only if it does.
//!
//! Two extra trials guard the corpus shape: it is non-empty, and every case is
//! well formed — its entry exists, it has a sibling `.rs` anchor (except a
//! `.fails` case), and a `.fails` marker is not blank.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use libtest_mimic::{Arguments, Failed, Trial};
use neoc_codegen_lua::TESTS_TABLE_DECL;
use neoc_compiler::{Target, transpile, transpile_project};

mod common;

/// A conformance backend: how to turn a case into a runnable program and run it.
struct TargetSpec {
    /// Short name shown after `::` in the test id (`lua`, `rust`).
    name: &'static str,
    /// Whether the toolchain this target needs is present. When false, the
    /// target's run trials are marked ignored rather than failed.
    available: bool,
    /// Whether this is the hand-written Rust anchor rather than a compiler
    /// backend. A rejection has no Rust analogue, so an anchor skips sad-path
    /// cases.
    anchor: bool,
    /// Produce the program text and run it under the toolchain. `Err` is a
    /// harness, compile or load failure, never a run-time rejection.
    run: fn(case: &Case) -> Result<Outcome, Failed>,
    /// The compiler [`Target`] and committed-output extension this backend emits
    /// (e.g. `(Target::Lua, "lua")`). When set, a `<case>::<target>::source`
    /// trial pins the emitted code against the readable `<case>.<ext>` file
    /// checked into the corpus. `None` for the Rust anchor, whose `.rs` is
    /// hand-written rather than emitted. This check needs no toolchain.
    emit: Option<(Target, &'static str)>,
}

/// How a case's program ended under a target.
struct Outcome {
    /// Everything the program printed before it finished or raised.
    stdout: String,
    /// The run-time error the program raised; `None` when it ran to completion.
    raised: Option<String>,
    /// The program text that ran, appended to failure reports.
    program: String,
}

/// Assert, for every module of `case`, that the live emit equals the committed
/// `<module>.<ext>` golden — the readable output file checked into the corpus
/// beside the module's `.neoc`.
fn check_source_golden(case: &Case, target: Target, ext: &str) -> Result<(), Failed> {
    for module in emit_case(case, target)? {
        let golden_path = module.source.with_extension(ext);
        if std::env::var_os("NEOC_BLESS").is_some() {
            std::fs::write(&golden_path, &module.output).map_err(|e| {
                Failed::from(format!(
                    "case `{}`: write {}: {e}",
                    case.name,
                    golden_path.display()
                ))
            })?;
            continue;
        }
        let committed = std::fs::read_to_string(&golden_path).map_err(|e| {
            Failed::from(format!(
                "case `{}`: read {}: {e}",
                case.name,
                golden_path.display()
            ))
        })?;
        if module.output != committed {
            return Err(Failed::from(format!(
                "case `{}`: emitted .{ext} differs from the committed golden {} — \
                 regenerate with `NEOC_BLESS=1 cargo test -p neoc-compiler --test conformance` ({target})\n\
                 --- emitted ---\n{}",
                case.name,
                golden_path.display(),
                module.output
            )));
        }
    }
    Ok(())
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

/// A conformance case: a single `<name>.neoc`, or a `<name>/` directory whose
/// `main.neoc` is the entry and whose sibling modules it `use`s.
#[derive(Clone)]
struct Case {
    /// The test-id stem: the file stem, or the directory name.
    name: String,
    /// The entry module's `.neoc` path.
    entry: PathBuf,
    /// Whether the case is a directory of modules (compiled as a project).
    project: bool,
}

impl Case {
    /// The entry's sibling file with the given extension (`stdout`, `rs`, `fails`).
    fn sibling(&self, ext: &str) -> PathBuf {
        self.entry.with_extension(ext)
    }

    /// The sad-path marker's text, when the case is expected to be rejected.
    fn expected_failure(&self) -> Option<String> {
        std::fs::read_to_string(self.sibling("fails"))
            .ok()
            .map(|text| text.trim().to_owned())
    }
}

/// One module of a case after emit.
struct EmittedModule {
    /// The module's `.neoc` source; its goldens sit beside it.
    source: PathBuf,
    /// The `require` name other modules use: the path under the case directory,
    /// dot-separated (`main` for the entry).
    name: String,
    /// The emitted program text.
    output: String,
}

/// Emit every module of `case` for `target`. A single file goes through
/// [`transpile`]; a directory goes through [`transpile_project`], so `use`
/// resolves exactly as it does for a real project. Fails the trial with the
/// diagnostics if compilation is not clean.
fn emit_case(case: &Case, target: Target) -> Result<Vec<EmittedModule>, Failed> {
    let unexpected = |diagnostics: &dyn std::fmt::Debug| {
        Failed::from(format!(
            "case `{}`: unexpected diagnostics: {diagnostics:#?}",
            case.name
        ))
    };
    if !case.project {
        let src = std::fs::read_to_string(&case.entry)
            .map_err(|e| Failed::from(format!("case `{}`: read .neoc: {e}", case.name)))?;
        let out = transpile(&src, target);
        if !out.ok() {
            return Err(unexpected(&out.diagnostics));
        }
        return Ok(vec![EmittedModule {
            source: case.entry.clone(),
            name: "main".to_owned(),
            output: out.output,
        }]);
    }
    let root = case
        .entry
        .parent()
        .and_then(|dir| dir.canonicalize().ok())
        .ok_or_else(|| Failed::from(format!("case `{}`: no case directory", case.name)))?;
    let project = transpile_project(&case.entry, target)
        .map_err(|e| Failed::from(format!("case `{}`: read project: {e}", case.name)))?;
    if !project.ok() {
        return Err(unexpected(&project.diagnostics));
    }
    project
        .files
        .into_iter()
        .map(|(dest, output)| {
            let source = dest.with_extension("neoc");
            let name = source
                .strip_prefix(&root)
                .map_err(|e| {
                    Failed::from(format!("case `{}`: module outside case: {e}", case.name))
                })?
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(".");
            Ok(EmittedModule {
                source,
                name,
                output,
            })
        })
        .collect()
}

/// rust: compile the case's `.rs` anchor with `rustc --edition 2021` and run it.
fn run_rust(case: &Case) -> Result<Outcome, Failed> {
    let rs_path = case.sibling("rs");
    let case = case.name.as_str();
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
    Ok(Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        raised: (!output.status.success())
            .then(|| String::from_utf8_lossy(&output.stderr).into_owned()),
        program: src,
    })
}

/// Epilogue run after `main()` so a `#[test]`-bearing case's run-trial actually
/// exercises §6.5 registration. The conformance run stands in for the §7 runtime:
/// it walks the module's `__tests` table in sorted-name order and invokes each
/// registered fn. Such a case's `main` therefore calls nothing itself — the walk
/// runs the tests — so dropping or mis-keying a registration changes observable
/// stdout and fails the trial, which the emit golden alone cannot catch. Guarded
/// on `__tests` existing, so it is a no-op for every case that emits no test
/// table (all but `6-06`). The Rust anchor has no runtime `__tests`; its `.rs`
/// calls the tests from `main` directly to reproduce the same stdout.
const TESTS_WALK_LUA: &str = r"if __tests then
  local __names = {}
  for __k in pairs(__tests) do __names[#__names + 1] = __k end
  table.sort(__names)
  for _, __k in ipairs(__names) do __tests[__k]() end
end
";

/// Lua prologue for a multi-module case: a `require` that serves the case's
/// own modules (registered in `__preload` by name) before deferring to the
/// runtime's `require` for `std:`/`lib:`/`vnd:` modules. Each module's chunk runs
/// once and its returned table is cached, as Lua's own `require` does.
const REQUIRE_SHIM_LUA: &str = r"local __runtime_require = require
local __preload, __loaded = {}, {}
require = function(name)
  if __loaded[name] ~= nil then return __loaded[name] end
  local chunk = __preload[name]
  if chunk == nil then return __runtime_require(name) end
  local module = chunk()
  __loaded[name] = module
  return module
end
";

/// lua: the emitted module is self-contained — inlined runtime, no `neoc-lib`.
/// Alias `println` to the built-in `print`, register every non-entry module with
/// the `require` shim, append the entry then `main()` and the `#[test]` walk, and
/// run in-process on `LuaJIT`. A directory case whose modules register `#[test]`
/// fns is refused: each module's `__tests` is local to its chunk, so the walk
/// could never reach them.
fn lua_program(case: &Case) -> Result<String, Failed> {
    let mut preload = String::new();
    let mut entry = String::new();
    for module in emit_case(case, Target::Lua)? {
        if case.project && module.output.lines().any(|line| line == TESTS_TABLE_DECL) {
            return Err(Failed::from(format!(
                "case `{}`: module `{}` declares #[test] fns, which a directory case \
                 cannot run — move them to a single-file case",
                case.name, module.name
            )));
        }
        if module.name == "main" {
            entry = module.output;
        } else {
            let _ = writeln!(
                preload,
                "__preload[{:?}] = function()\n{}\nend",
                module.name, module.output
            );
        }
    }
    // A project entry is a requireable library like its siblings: it closes with
    // an export table, so `main` must be `pub` and is called through it.
    let run = if case.project {
        format!("local __entry = (function()\n{entry}\nend)()\n__entry.main()\n")
    } else {
        format!("{entry}\nmain()\n{TESTS_WALK_LUA}")
    };
    Ok(format!("println = print\n{REQUIRE_SHIM_LUA}{preload}{run}"))
}

fn run_lua(case: &Case) -> Result<Outcome, Failed> {
    let program = lua_program(case)?;
    let run = common::run_lua_capturing(&program).map_err(|e| {
        Failed::from(format!(
            "case `{}`: lua failed to load:\n{e}\n--- program ---\n{program}",
            case.name
        ))
    })?;
    Ok(Outcome {
        stdout: run.stdout,
        raised: run.raised,
        program,
    })
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

/// Every case in the corpus, sorted for a stable iteration order: each
/// top-level `.neoc` file, and each subdirectory (entered at its `main.neoc`).
fn cases(dir: &Path) -> Vec<Case> {
    let mut cases: Vec<Case> = std::fs::read_dir(dir)
        .expect("read corpus dir")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?.to_owned();
            if path.is_dir() {
                Some(Case {
                    name: path.file_name()?.to_str()?.to_owned(),
                    entry: path.join("main.neoc"),
                    project: true,
                })
            } else if path.extension().is_some_and(|ext| ext == "neoc") {
                Some(Case {
                    name,
                    entry: path,
                    project: false,
                })
            } else {
                None
            }
        })
        .collect();
    cases.sort_by(|a, b| a.name.cmp(&b.name));
    cases
}

/// Build the per-case × target trials plus the corpus-shape guards.
/// The conformance backends: the `rust` anchor plus the sole target, `lua`. Each
/// `::source` golden pin needs no toolchain; the `lua` run trial executes
/// in-process, and the `rust` run trial is ignored when `rustc` is absent.
fn target_specs() -> [TargetSpec; 2] {
    [
        TargetSpec {
            name: "rust",
            available: binary_present("rustc"),
            anchor: true,
            run: run_rust,
            emit: None,
        },
        TargetSpec {
            name: "lua",
            available: true,
            anchor: false,
            run: run_lua,
            emit: Some((Target::Lua, "lua")),
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

    // Guard: corpus shape (see `check_well_formed`).
    let guarded: Vec<Case> = cases.iter().map(Case::clone).collect();
    trials.push(Trial::test("corpus::every_case_well_formed", move || {
        check_well_formed(&guarded)
    }));

    for case in cases {
        let case = std::sync::Arc::new(case);
        let sad_path = case.expected_failure();

        for target in &targets {
            // Source golden: the live emit equals the committed readable output
            // file. Needs only the compiler, so it runs without any interpreter.
            if let Some((emit_target, ext)) = target.emit {
                let case = std::sync::Arc::clone(&case);
                trials.push(Trial::test(
                    format!("{}::{}::source", case.name, target.name),
                    move || check_source_golden(&case, emit_target, ext),
                ));
            }

            // A sad-path case has no Rust anchor: a rejection is not a program.
            if sad_path.is_some() && target.anchor {
                continue;
            }

            // Run: transpile, run under the toolchain, assert stdout (or, for a
            // sad path, the rejection). Ignored when the toolchain is absent.
            let run = target.run;
            let case = std::sync::Arc::clone(&case);
            let sad_path = sad_path.clone();
            let trial = Trial::test(format!("{}::{}", case.name, target.name), move || {
                judge(run, &case, sad_path.as_deref())
            })
            .with_ignored_flag(!target.available);
            trials.push(trial);
        }
    }

    trials
}

/// Every entry module exists, every case that is not a sad path has a sibling
/// `.rs` so the two halves can't drift, and no `.fails` marker is blank (an
/// empty fragment is contained in every message, so it would admit any crash).
fn check_well_formed(cases: &[Case]) -> Result<(), Failed> {
    for case in cases {
        let name = &case.name;
        if !case.entry.exists() {
            return Err(Failed::from(format!(
                "case `{name}`: missing entry module {}",
                case.entry.display()
            )));
        }
        let fails = case.expected_failure();
        if fails.as_deref().is_some_and(str::is_empty) {
            return Err(Failed::from(format!(
                "case `{name}`: empty .fails marker — it must name the expected error"
            )));
        }
        let rs = case.sibling("rs");
        if fails.is_none() && !rs.exists() {
            return Err(Failed::from(format!(
                "case `{name}`: missing sibling Rust anchor {}",
                rs.display()
            )));
        }
    }
    Ok(())
}

/// Run `case` under a target and judge the outcome: a sad-path case (`want` is
/// `Some`) must be rejected, any other must complete.
fn judge(
    run: fn(&Case) -> Result<Outcome, Failed>,
    case: &Case,
    want: Option<&str>,
) -> Result<(), Failed> {
    let outcome = run(case)?;
    match want {
        Some(want) => assert_rejected(case, &outcome, want),
        None => assert_completed(case, &outcome),
    }
}

/// Assert `stdout` equals the case's `.stdout` golden, ignoring trailing newlines.
fn assert_stdout(case: &Case, stdout: &str) -> Result<(), Failed> {
    let expected = std::fs::read_to_string(case.sibling("stdout"))
        .map_err(|e| Failed::from(format!("case `{}`: read .stdout: {e}", case.name)))?;
    let got = stdout.trim_end_matches('\n');
    let want = expected.trim_end_matches('\n');
    if got == want {
        Ok(())
    } else {
        Err(Failed::from(format!(
            "case `{}`: stdout mismatch\n--- expected ---\n{want}\n--- got ---\n{got}",
            case.name
        )))
    }
}

/// Assert a happy-path run completed and printed the `.stdout` golden.
fn assert_completed(case: &Case, outcome: &Outcome) -> Result<(), Failed> {
    if let Some(raised) = &outcome.raised {
        return Err(Failed::from(format!(
            "case `{}`: run failed:\n{raised}\n--- program ---\n{}",
            case.name, outcome.program
        )));
    }
    assert_stdout(case, &outcome.stdout)
}

/// Assert a sad-path run raised an error whose message contains `want`, after
/// printing exactly the `.stdout` golden. Only the raised message is searched —
/// never the program text, which may contain the marker anywhere.
fn assert_rejected(case: &Case, outcome: &Outcome, want: &str) -> Result<(), Failed> {
    let Some(raised) = &outcome.raised else {
        return Err(Failed::from(format!(
            "case `{}`: expected a rejection containing `{want}`, but the program ran:\n{}",
            case.name, outcome.stdout
        )));
    };
    if !raised.contains(want) {
        return Err(Failed::from(format!(
            "case `{}`: rejection does not contain `{want}`:\n{raised}\n--- program ---\n{}",
            case.name, outcome.program
        )));
    }
    assert_stdout(case, &outcome.stdout)
}

/// A scratch case on disk: `files` are written under a fresh temp directory,
/// the first being the entry module.
fn scratch_case(project: bool, files: &[(&str, &str)]) -> Case {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("neoc_conf_self_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch case dir");
    for (name, text) in files {
        std::fs::write(dir.join(name), text).expect("write scratch file");
    }
    Case {
        name: format!("scratch-{n}"),
        entry: dir.join(files[0].0),
        project,
    }
}

/// A self-test trial: passes only when `check` fails with a message containing
/// `needle`. A harness that stops rejecting turns the corpus into a false PASS
/// that stays green, so each guard is pinned by a case it must refuse.
fn must_fail(
    name: &str,
    needle: &'static str,
    check: impl FnOnce() -> Result<(), Failed> + Send + 'static,
) -> Trial {
    Trial::test(format!("harness::{name}"), move || match check() {
        Ok(()) => Err(Failed::from(format!(
            "expected the harness to fail with `{needle}`, but it passed"
        ))),
        Err(e) if e.message().is_some_and(|m| m.contains(needle)) => Ok(()),
        Err(e) => Err(Failed::from(format!(
            "expected a failure containing `{needle}`, got: {}",
            e.message().unwrap_or_default()
        ))),
    })
}

/// A fixed outcome for the `assert_*` self-tests.
fn outcome(stdout: &str, raised: Option<&str>, program: &str) -> Outcome {
    Outcome {
        stdout: stdout.to_owned(),
        raised: raised.map(str::to_owned),
        program: program.to_owned(),
    }
}

/// Self-tests for the harness's own assertion logic: each one feeds a guard
/// input it must refuse (plus one control it must accept) and passes only if
/// the guard fails with the expected message.
fn self_test_trials() -> Vec<Trial> {
    const OK_MAIN: &str =
        "extern {\n    fn println(s: string);\n}\n\nfn main() {\n    println(\"hi\");\n}\n";
    let mut trials = Vec::new();

    // Control: a matching rejection with the golden stdout is accepted.
    trials.push(Trial::test("harness::rejection_accepted", || {
        let case = scratch_case(false, &[("c.neoc", OK_MAIN), ("c.stdout", "out\n")]);
        assert_rejected(&case, &outcome("out", Some("boom: bad"), ""), "boom")
    }));

    // The marker appears only in the program text and stdout, never in the
    // raised message: the sad path must search the rejection alone.
    trials.push(must_fail(
        "rejection_marker_only_in_program",
        "rejection does not contain",
        || {
            let case = scratch_case(false, &[("c.neoc", OK_MAIN), ("c.stdout", "has-marker\n")]);
            let out = outcome("has-marker", Some("something else"), "-- has-marker");
            assert_rejected(&case, &out, "has-marker")
        },
    ));

    trials.push(must_fail(
        "blank_fails_marker",
        "empty .fails marker",
        || {
            let case = scratch_case(false, &[("c.neoc", OK_MAIN), ("c.fails", "  \n")]);
            check_well_formed(&[case])
        },
    ));

    trials.push(must_fail(
        "missing_rust_anchor",
        "missing sibling Rust anchor",
        || {
            let case = scratch_case(false, &[("c.neoc", OK_MAIN)]);
            check_well_formed(&[case])
        },
    ));

    trials.push(must_fail("directory_case_declares_tests", "declares #[test] fns", || {
        let main = "extern {\n    fn println(s: string);\n}\n\n#[test]\nfn checks() {\n    println(\"t\");\n}\n\npub fn main() {}\n";
        let case = scratch_case(true, &[("main.neoc", main)]);
        lua_program(&case).map(|_| ())
    }));

    trials.push(must_fail(
        "rejection_stdout_mismatch",
        "stdout mismatch",
        || {
            let case = scratch_case(false, &[("c.neoc", OK_MAIN), ("c.stdout", "expected\n")]);
            assert_rejected(&case, &outcome("actual", Some("boom"), ""), "boom")
        },
    ));

    // A `.fails` marker on a program that runs clean is not a rejection.
    trials.push(must_fail(
        "fails_on_clean_run",
        "expected a rejection",
        || {
            let case = scratch_case(
                false,
                &[
                    ("c.neoc", OK_MAIN),
                    ("c.fails", "boom\n"),
                    ("c.stdout", "hi\n"),
                ],
            );
            judge(run_lua, &case, Some("boom"))
        },
    ));

    // A compile error never satisfies a `.fails` marker, even when the
    // diagnostics happen to contain the marker's text.
    trials.push(must_fail(
        "fails_on_compile_error",
        "unexpected diagnostics",
        || {
            let bad = "fn main() {\n    let x: number = \"text\";\n}\n";
            let case = scratch_case(
                false,
                &[("c.neoc", bad), ("c.fails", "number\n"), ("c.stdout", "")],
            );
            judge(run_lua, &case, Some("number"))
        },
    ));

    trials
}

fn main() {
    let args = Arguments::from_args();
    let mut trials = collect_trials();
    trials.extend(self_test_trials());
    libtest_mimic::run(&args, trials).exit();
}
