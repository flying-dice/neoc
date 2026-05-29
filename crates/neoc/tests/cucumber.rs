//! Executable CLI spec for the `neoc` binary.
//!
//! The Gherkin features in `tooling/cli/` are the behavioural contract for the
//! command-line surface: `check` accepts a well-formed program and refuses a
//! broken one, `build` transpiles a clean entry to Lua next to the source, and
//! `run --eval` executes an inline snippet — each reported in neoc terms. The
//! step definitions drive the *built* binary as a subprocess (the same way
//! `tests/cli.rs` does), so the features *are* the spec, mirroring
//! `crates/neoc-fmt/tests/cucumber.rs` and `lang/CONFORMANCE/`.
//!
//! Scenarios tagged `@pending` describe behaviour that is specified but not yet
//! verifiable here; the runner filters them out so the suite stays green. Drop
//! the tag once the behaviour lands.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;
use std::process::{Command, Output};

use cucumber::{World, gherkin::Step, given, then, when};
use tempfile::TempDir;

/// The built `neoc` binary, supplied by Cargo to the test target.
const BIN: &str = env!("CARGO_BIN_EXE_neoc");

/// State threaded through one scenario: a private working directory the files
/// are written into, and the outcome of the last `neoc` invocation.
#[derive(Debug, Default, World)]
struct Cli {
    /// The scenario's scratch directory; commands run with this as their cwd so
    /// relative paths and glob expansion resolve here, not the test crate.
    dir: Option<TempDir>,
    /// The captured output of the last `When I run` step.
    output: Option<Output>,
}

impl Cli {
    /// The scenario's working directory, created on first use.
    fn working_dir(&mut self) -> &Path {
        self.dir
            .get_or_insert_with(|| tempfile::tempdir().expect("create a temp working directory"))
            .path()
    }

    /// The captured output of the last run, panicking if no command has run.
    fn last(&self) -> &Output {
        self.output
            .as_ref()
            .unwrap_or_else(|| panic!("no `neoc` command has run yet"))
    }

    /// The last run's stderr as text.
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.last().stderr).into_owned()
    }

    /// The last run's stdout as text.
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.last().stdout).into_owned()
    }
}

#[given(regex = r#"^a file "([^"]+)" with:$"#)]
fn given_file(world: &mut Cli, name: String, step: &Step) {
    let contents = docstring(step).trim_start_matches('\n');
    let dir = world.working_dir().to_path_buf();
    std::fs::write(dir.join(&name), contents).unwrap_or_else(|e| panic!("writing {name}: {e}"));
}

#[when(regex = r#"^I run "neoc (.+)"$"#)]
fn run_neoc(world: &mut Cli, args: String) {
    let argv = split_args(&args);
    let dir = world.working_dir().to_path_buf();
    let output = Command::new(BIN)
        .args(&argv)
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| panic!("spawning `neoc {args}`: {e}"));
    world.output = Some(output);
}

#[then("it exits successfully")]
fn exits_successfully(world: &mut Cli) {
    let out = world.last();
    assert!(
        out.status.success(),
        "expected a zero exit, got {:?}\nstderr:\n{}\nstdout:\n{}",
        out.status.code(),
        world.stderr(),
        world.stdout(),
    );
}

#[then("it exits with an error")]
fn exits_with_error(world: &mut Cli) {
    let out = world.last();
    assert!(
        !out.status.success(),
        "expected a non-zero exit, but it succeeded\nstdout:\n{}",
        world.stdout(),
    );
}

#[then(regex = r#"^stderr contains "([^"]*)"$"#)]
fn stderr_contains(world: &mut Cli, needle: String) {
    let stderr = world.stderr();
    assert!(
        stderr.contains(&needle),
        "stderr did not contain {needle:?}\nstderr:\n{stderr}",
    );
}

#[then(regex = r#"^stdout contains "([^"]*)"$"#)]
fn stdout_contains(world: &mut Cli, needle: String) {
    let stdout = world.stdout();
    assert!(
        stdout.contains(&needle),
        "stdout did not contain {needle:?}\nstdout:\n{stdout}",
    );
}

#[then(regex = r#"^a non-empty file "([^"]+)" exists$"#)]
fn non_empty_file_exists(world: &mut Cli, name: String) {
    let path = world.working_dir().join(&name);
    assert!(path.is_file(), "expected {name} to exist");
    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {name}: {e}"));
    assert!(!contents.is_empty(), "expected {name} to be non-empty");
}

#[then(regex = r#"^no file "([^"]+)" exists$"#)]
fn no_file_exists(world: &mut Cli, name: String) {
    let path = world.working_dir().join(&name);
    assert!(!path.exists(), "expected {name} not to exist, but it does",);
}

/// Splits a `neoc` argument line into argv. Tokens split on whitespace, except
/// that everything after `-e`/`--eval` is kept as one argument: an inline
/// snippet (`fn main() -> number { 1 + 2 }`) is a single program string, the
/// same way `tests/cli.rs` passes it as one `.arg(...)`.
fn split_args(line: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        let (token, tail) = match rest.split_once(char::is_whitespace) {
            Some((head, tail)) => (head, tail.trim_start()),
            None => (rest, ""),
        };
        argv.push(token.to_string());
        if token == "-e" || token == "--eval" {
            if !tail.is_empty() {
                argv.push(tail.to_string());
            }
            break;
        }
        rest = tail;
    }
    argv
}

/// The docstring attached to a step, panicking with the step text if absent.
fn docstring(step: &Step) -> &str {
    let Some(doc) = step.docstring.as_deref() else {
        panic!("step {:?} requires a docstring", step.value);
    };
    doc
}

fn main() {
    // Features live at the repo top (`tooling/cli/`), resolved from this crate's
    // manifest directory — a test's working directory is the crate, not the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("cli");
    futures::executor::block_on(Cli::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
