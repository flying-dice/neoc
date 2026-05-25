//! Integration tests driving the unified `neoc` binary: the compiler
//! subcommands (`build`/`check`/`fmt`/`lsp`) and `run` of a `.neoc` entry
//! (in-process transpile + execute, with runtime faults mapped to neoc lines).

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

const BIN: &str = env!("CARGO_BIN_EXE_neoc");

const VALID: &str = "ext fn println(n: number);\nfn main() {\n  println(1 + 2);\n}\n";
const BROKEN: &str = "fn main( {\n";

fn write(dir: &Path, name: &str, src: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    path
}

#[test]
fn build_writes_lua_next_to_source() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "foo.neoc", VALID);

    let status = Command::new(BIN).arg("build").arg(&src).status().unwrap();
    assert!(status.success());

    let lua = dir.path().join("foo.lua");
    assert!(lua.is_file(), "expected {} to exist", lua.display());
    assert!(!fs::read_to_string(&lua).unwrap().is_empty());
}

#[test]
fn build_with_output_dir_keeps_base_name() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "foo.neoc", VALID);
    let out = dir.path().join("out");

    let status = Command::new(BIN)
        .arg("build")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());

    assert!(out.join("foo.lua").is_file());
    assert!(!dir.path().join("foo.lua").exists());
}

#[test]
fn build_glob_matches_multiple_files() {
    let dir = tempdir().unwrap();
    write(dir.path(), "a.neoc", VALID);
    write(dir.path(), "b.neoc", VALID);

    let pattern = format!("{}/*.neoc", dir.path().display());
    let status = Command::new(BIN)
        .arg("build")
        .arg(&pattern)
        .status()
        .unwrap();
    assert!(status.success());

    assert!(dir.path().join("a.lua").is_file());
    assert!(dir.path().join("b.lua").is_file());
}

#[test]
fn build_skips_output_on_diagnostic_and_exits_nonzero() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "bad.neoc", BROKEN);

    let out = Command::new(BIN).arg("build").arg(&src).output().unwrap();
    assert!(!out.status.success());
    assert!(!dir.path().join("bad.lua").exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad.neoc"), "stderr: {stderr}");
}

#[test]
fn build_multi_file_project_emits_every_module_next_to_source() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "util.neoc",
        "pub fn add(a: number, b: number) -> number { a + b }\n",
    );
    let entry = write(
        dir.path(),
        "main.neoc",
        "use util::add;\nfn main() -> number { add(2, 3) }\n",
    );

    let status = Command::new(BIN).arg("build").arg(&entry).status().unwrap();
    assert!(status.success());

    let main_lua = dir.path().join("main.lua");
    let util_lua = dir.path().join("util.lua");
    assert!(main_lua.is_file(), "expected main.lua");
    assert!(
        util_lua.is_file(),
        "expected util.lua emitted from the graph"
    );
    assert!(fs::read_to_string(&main_lua).unwrap().contains("require"));
}

#[test]
fn build_reports_private_import_and_writes_nothing() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "util.neoc",
        "fn secret(x: number) -> number { x }\n",
    );
    let entry = write(
        dir.path(),
        "main.neoc",
        "use util::secret;\nfn main() -> number { secret(1) }\n",
    );

    let out = Command::new(BIN).arg("build").arg(&entry).output().unwrap();
    assert!(!out.status.success());
    assert!(!dir.path().join("main.lua").exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("`secret` is private in module `util`"),
        "stderr: {stderr}"
    );
}

#[test]
fn check_clean_exits_zero() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "ok.neoc", VALID);

    let status = Command::new(BIN).arg("check").arg(&src).status().unwrap();
    assert!(status.success());
    assert!(!dir.path().join("ok.lua").exists());
}

#[test]
fn check_diagnostic_exits_nonzero() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "bad.neoc", BROKEN);

    let out = Command::new(BIN).arg("check").arg(&src).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad.neoc"), "stderr: {stderr}");
}

const MESSY: &str = "struct   Point{x:number,y:number}\nfn main(){let p=Point{x:1,y:2};}\n";

#[test]
fn fmt_rewrites_file_in_place_and_is_a_no_op_on_second_run() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "messy.neoc", MESSY);

    let status = Command::new(BIN).arg("fmt").arg(&src).status().unwrap();
    assert!(status.success());

    let formatted = fs::read_to_string(&src).unwrap();
    assert_ne!(formatted, MESSY, "expected the file to be rewritten");
    assert!(formatted.contains("struct Point {\n    x: number,\n    y: number,\n}"));
    assert!(formatted.ends_with('\n'));

    // A second run leaves the now-canonical file untouched.
    let status = Command::new(BIN).arg("fmt").arg(&src).status().unwrap();
    assert!(status.success());
    let again = fs::read_to_string(&src).unwrap();
    assert_eq!(again, formatted, "second run was not a no-op");
}

#[test]
fn fmt_check_exits_nonzero_for_unformatted_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "messy.neoc", MESSY);

    let out = Command::new(BIN)
        .arg("fmt")
        .arg("--check")
        .arg(&src)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not formatted"), "stderr: {stderr}");
    // --check must not touch the file.
    assert_eq!(fs::read_to_string(&src).unwrap(), MESSY);
}

#[test]
fn fmt_check_exits_zero_when_already_formatted() {
    const CANONICAL: &str = "ext fn println(n: number);\n\nfn main() {\n    println(1 + 2);\n}\n";
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "ok.neoc", CANONICAL);

    let status = Command::new(BIN)
        .arg("fmt")
        .arg("--check")
        .arg(&src)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn fmt_parse_error_exits_nonzero() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "bad.neoc", BROKEN);

    let out = Command::new(BIN).arg("fmt").arg(&src).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad.neoc"), "stderr: {stderr}");
}

#[test]
fn lsp_starts_and_exits_cleanly_when_stdin_closes() {
    // `neoc lsp` runs the language server over stdio. With a closed stdin the
    // client connection ends immediately, so the server shuts down with exit 0.
    let out = Command::new(BIN)
        .arg("lsp")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// `print` is the built-in the sandboxed VM exposes (unlike the conformance
// harness's `println`), so a runnable `.neoc` binds it as an `ext`.
const RUNNABLE: &str = "ext fn print(s: string);\nfn main() {\n  print(\"ran neoc\");\n}\n";

#[test]
fn run_neoc_transpiles_and_executes() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "app.neoc", RUNNABLE);

    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ran neoc");
    // No intermediate file is written next to the source.
    assert!(!dir.path().join("app.lua").exists());
}

#[test]
fn run_neoc_type_error_reports_neoc_diagnostic_not_lua() {
    let dir = tempdir().unwrap();
    let src = write(
        dir.path(),
        "bad.neoc",
        "fn main() {\n  let x: string = 42;\n}\n",
    );

    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("type:"),
        "expected a neoc type diagnostic, got: {stderr}"
    );
}

// A program that compiles cleanly but faults at runtime: `crash()` (neoc line
// 4) calls an unbound name, so the VM raises `attempt to call a nil value`. The
// offending call sits on neoc line 4; the emitted Lua puts it on a different
// line, so a correct source map must report line 4, not the Lua line. (#55, #56)
const RUNTIME_FAULT: &str = "\
ext fn print(s: string);
fn crash() {
  print(\"in crash\");
  let n = explode();
}
ext fn explode() -> number;
fn main() {
  crash();
}
";

#[test]
fn run_neoc_runtime_fault_reports_neoc_line_not_lua_line() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "fault.neoc", RUNTIME_FAULT);

    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);

    // The fault names the `.neoc` file and the neoc line of the offending call
    // (`let n = explode();` on line 4), not the emitted-Lua line.
    assert!(
        stderr.contains("fault.neoc\"]:4:"),
        "expected the fault at fault.neoc:4 (neoc line), got: {stderr}"
    );
    // The emitted Lua puts `explode()` on line 3 (no `ext`/header lines emit), so
    // an unmapped report would have said `:3:` — assert that leaked Lua line is
    // gone from the chunk reference.
    assert!(
        !stderr.contains("fault.neoc\"]:3:"),
        "Lua line 3 leaked into the report; map not applied: {stderr}"
    );
}

#[test]
fn run_neoc_multi_module_is_rejected_with_pointer_to_build() {
    // `run` executes a single file; a program that `use`s another module lowers
    // to `require("util")`, which has no resolver under the runtime's
    // `namespace:name` require — so it's rejected up front (not at a confusing
    // runtime require failure) with a pointer to `neoc build`.
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "util.neoc",
        "pub fn add(a: number, b: number) -> number { a + b }\n",
    );
    let entry = write(
        dir.path(),
        "app.neoc",
        "use util::add;\nfn main() -> number { add(2, 3) }\n",
    );

    let out = Command::new(BIN).arg("run").arg(&entry).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("imports other modules") && stderr.contains("neoc build"),
        "expected a multi-module rejection pointing at `neoc build`, got: {stderr}"
    );
}
