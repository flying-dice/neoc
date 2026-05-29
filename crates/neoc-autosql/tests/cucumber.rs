//! Executable generation spec for `neoc-autosql`.
//!
//! The Gherkin features in `tooling/autosql/` are the behavioural contract for
//! the SQL→neoc data-access generator: a schema and a set of single-query files
//! become one `ports.neoc`, one `migrate.neoc`, and one typed `.neoc` file per
//! query. The step definitions drive `neoc_autosql::generate_project` directly,
//! so the features *are* the spec — mirroring `crates/neoc-lsp/tests/cucumber.rs`
//! and the crate's own `tests/valid_neoc.rs`.
//!
//! Each scenario writes its DDL and queries into a throwaway `tempfile::tempdir`
//! laid out as a neoc project (`src/main/sql/{ddl,dml}`), then reads them back
//! and runs the generator — keeping the filesystem flavour of the real
//! convention-driven workflow while driving the real (in-memory) API.
//!
//! Scenarios tagged `@pending` describe specified-but-unverified behaviour; the
//! runner filters them out so the suite stays green. Drop the tag once the
//! behaviour is pinned down.

// cucumber parses each captured step argument into an owned value — it has no
// borrow to lend — so capturing step functions take their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::fs;
use std::path::Path;
use std::str::FromStr as _;

use cucumber::{World, gherkin::Step, given, then, when};
use neoc_autosql::{Dialect, GeneratedFile, Migration, Query, generate_project};
use tempfile::TempDir;

/// State threaded through one scenario: the throwaway project tree, the DDL and
/// query files written into it, and the outcome of the last generation — either
/// the generated files or the error's SQL/neoc-flavoured message.
#[derive(Debug, World)]
struct Gen {
    /// The throwaway project root; its `src/main/sql/{ddl,dml}` tree holds the
    /// SQL written by the `Given` steps. Dropped (deleted) with the world.
    project: TempDir,
    /// Migrations written so far, in `(order, name)` declaration order.
    migrations: Vec<Migration>,
    /// Queries written so far, in declaration order.
    queries: Vec<Query>,
    /// The generated files from the last `When I generate …`, if it succeeded.
    generated: Option<Vec<GeneratedFile>>,
    /// The error message from the last `When I generate …`, if it failed.
    error: Option<String>,
}

impl Default for Gen {
    fn default() -> Self {
        let project = tempfile::tempdir().expect("a writable temp dir");
        for sub in ["src/main/sql/ddl", "src/main/sql/dml"] {
            fs::create_dir_all(project.path().join(sub)).expect("create the SQL tree");
        }
        Gen {
            project,
            migrations: Vec::new(),
            queries: Vec::new(),
            generated: None,
            error: None,
        }
    }
}

impl Gen {
    /// The `src/main/sql` directory of this scenario's project.
    fn sql_dir(&self) -> std::path::PathBuf {
        self.project.path().join("src/main/sql")
    }
}

#[given("the schema:")]
fn given_schema(world: &mut Gen, step: &Step) {
    let sql = docstring(step).trim().to_string();
    let order = u32::try_from(world.migrations.len()).unwrap_or(0) + 1;
    let name = format!("init_{order}");
    let path = world
        .sql_dir()
        .join("ddl")
        .join(format!("{order:04}-{name}.sql"));
    fs::write(&path, &sql).expect("write the DDL file");
    // Read it back, mirroring the convention-driven workflow that loads
    // `src/main/sql/ddl/*` from disk before generating.
    let sql = fs::read_to_string(&path).expect("read the DDL file");
    world.migrations.push(Migration { order, name, sql });
}

#[given(regex = r#"^the query "([^"]+)":$"#)]
fn given_query(world: &mut Gen, name: String, step: &Step) {
    let sql = docstring(step).trim().to_string();
    let path = world.sql_dir().join("dml").join(format!("{name}.sql"));
    fs::write(&path, &sql).expect("write the DML file");
    let sql = fs::read_to_string(&path).expect("read the DML file");
    world.queries.push(Query { name, sql });
}

#[when("I generate for sqlite")]
fn generate_sqlite(world: &mut Gen) {
    generate(world, Dialect::Sqlite);
}

#[when("I generate for postgres")]
fn generate_postgres(world: &mut Gen) {
    generate(world, Dialect::Postgres);
}

#[when("I generate for mysql")]
fn generate_mysql(world: &mut Gen) {
    generate(world, Dialect::Mysql);
}

#[when(regex = r#"^I generate for the dialect "([^"]*)"$"#)]
fn generate_for_named_dialect(world: &mut Gen, name: String) {
    // An unrecognised dialect name is refused before generation even starts.
    match Dialect::from_str(&name) {
        Ok(dialect) => generate(world, dialect),
        Err(message) => {
            world.error = Some(message);
            world.generated = None;
        }
    }
}

/// Runs the generator and records the outcome on the world.
fn generate(world: &mut Gen, dialect: Dialect) {
    match generate_project(&world.migrations, &world.queries, dialect) {
        Ok(files) => {
            world.generated = Some(files);
            world.error = None;
        }
        Err(err) => {
            world.error = Some(err.to_string());
            world.generated = None;
        }
    }
}

#[then(regex = r#"^it generates a file "([^"]+)"$"#)]
fn generates_a_file(world: &mut Gen, file_name: String) {
    let files = generated(world);
    assert!(
        files.iter().any(|f| f.relative_path.ends_with(&file_name)),
        "no generated file `{file_name}`; got {:?}",
        relative_paths(files),
    );
}

#[then(regex = r#"^it generates the file "([^"]+)" under "([^"]+)"$"#)]
fn generates_file_under(world: &mut Gen, file_name: String, dir: String) {
    let files = generated(world);
    let expected = Path::new(&dir).join(&file_name);
    assert!(
        files.iter().any(|f| f.relative_path == expected),
        "no generated file at `{}`; got {:?}",
        expected.display(),
        relative_paths(files),
    );
}

#[then(regex = r#"^the generated "([^"]+)" contains:$"#)]
fn generated_contains_doc(world: &mut Gen, file_name: String, step: &Step) {
    let expected = docstring(step).trim();
    let contents = file_contents(world, &file_name);
    assert!(
        contents.contains(expected),
        "`{file_name}` missing expected text\n  expected substring:\n{expected}\n  actual contents:\n{contents}",
    );
}

#[then(regex = r#"^the generated "([^"]+)" contains "([^"]*)"$"#)]
fn generated_contains_inline(world: &mut Gen, file_name: String, needle: String) {
    let contents = file_contents(world, &file_name);
    assert!(
        contents.contains(&needle),
        "`{file_name}` missing {needle:?}\n  actual contents:\n{contents}",
    );
}

#[then(regex = r#"^the generated "([^"]+)" does not contain "([^"]*)"$"#)]
fn generated_excludes(world: &mut Gen, file_name: String, needle: String) {
    let contents = file_contents(world, &file_name);
    assert!(
        !contents.contains(&needle),
        "`{file_name}` unexpectedly contains {needle:?}\n  actual contents:\n{contents}",
    );
}

#[then("generation is refused")]
fn generation_refused(world: &mut Gen) {
    assert!(
        world.error.is_some(),
        "expected generation to be refused, but it produced {} files",
        world.generated.as_ref().map_or(0, Vec::len),
    );
}

#[then(regex = r#"^the error mentions "([^"]*)"$"#)]
fn error_mentions(world: &mut Gen, fragment: String) {
    let message = world
        .error
        .as_deref()
        .unwrap_or_else(|| panic!("expected an error, got files {:?}", world.generated));
    assert!(
        message.contains(&fragment),
        "error message missing {fragment:?}\n  actual: {message:?}",
    );
}

/// The generated files of the last successful generation, or a panic carrying
/// the error if it failed.
fn generated(world: &Gen) -> &[GeneratedFile] {
    world
        .generated
        .as_deref()
        .unwrap_or_else(|| panic!("expected generated files, got error {:?}", world.error))
}

/// The contents of a generated file by its file name (last path segment).
fn file_contents<'a>(world: &'a Gen, file_name: &str) -> &'a str {
    generated(world)
        .iter()
        .find(|f| f.relative_path.ends_with(file_name))
        .unwrap_or_else(|| {
            panic!(
                "no generated file `{file_name}`; got {:?}",
                relative_paths(generated(world)),
            )
        })
        .contents
        .as_str()
}

/// The relative paths of a file set, for assertion messages.
fn relative_paths(files: &[GeneratedFile]) -> Vec<String> {
    files
        .iter()
        .map(|f| f.relative_path.display().to_string())
        .collect()
}

/// The docstring attached to a step, panicking with the step text if absent.
fn docstring(step: &Step) -> &str {
    let Some(doc) = step.docstring.as_deref() else {
        panic!("step {:?} requires a docstring", step.value);
    };
    doc
}

fn main() {
    // Features live at the repo top (`tooling/autosql/`), resolved from this
    // crate's manifest directory — a test's working directory is the crate, not
    // the root.
    let features = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tooling")
        .join("autosql");
    futures::executor::block_on(Gen::filter_run(features, |_feature, _rule, scenario| {
        !scenario.tags.iter().any(|tag| tag == "pending")
    }));
}
