//! The modules as a script sees them: every `tests/scripts/<module>.luau` runs on an engine of its own and must
//! pass.
//!
//! A script asserts what it checks. It may also return a table of plain values, its answers: they must equal
//! `tests/scripts/<module>.json`, the answers written down. A script whose answers are the platform's (paths,
//! processes, terminals) has a file per platform family instead: `<module>.unix.json` and `<module>.windows.json`.
//! Where an answer changes on purpose, run the test with `NEOC_UPDATE_ANSWERS=1` and review the difference: it
//! rewrites the file of every script that returns answers (for the platform it runs on).
//!
//! The scripts are written in Luau's strict mode and are type-checked against the generated declarations by
//! `tests/strict_luau.rs`.

use std::path::{Path, PathBuf};

use neoc_vm::mlua::{LuaSerdeExt, Value};

/// The platform families a script may have answers of its own for; the first is the one this test runs on.
const FAMILIES: [&str; 2] = if cfg!(windows) { ["windows", "unix"] } else { ["unix", "windows"] };

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("scripts")
}

/// Every script, as its module's name and its source, sorted by name.
fn scripts() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = std::fs::read_dir(dir())
        .expect("the scripts directory")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(".luau")?;
            Some((name.to_owned(), std::fs::read_to_string(entry.path()).expect("a script")))
        })
        .collect();
    found.sort();
    found
}

/// What the script returned as plain values, `None` when it returned nothing.
async fn run(code: &str) -> Result<Option<serde_json::Value>, String> {
    let lua = neoc_vm::make_engine().map_err(|e| e.to_string())?;
    let value: Value = lua.load(code).eval_async().await.map_err(|e| e.to_string())?;
    if value.is_nil() {
        return Ok(None);
    }
    lua.from_value(value).map(Some).map_err(|e| e.to_string())
}

/// The name of the file of a script's answers: this platform family's, where the script has a file per family.
fn answers_file(name: &str) -> String {
    let per_family = FAMILIES.iter().any(|family| dir().join(format!("{name}.{family}.json")).is_file());
    if per_family { format!("{name}.{}.json", FAMILIES[0]) } else { format!("{name}.json") }
}

/// The answers as the text of their file.
fn written(answers: &serde_json::Value) -> String {
    serde_json::to_string_pretty(answers).expect("plain values are JSON") + "\n"
}

/// What is wrong with a script's answers against its file, if anything. With `update`, the file is made to say
/// what the script answered instead.
fn check_answers(name: &str, answers: Option<&serde_json::Value>, update: bool) -> Result<(), String> {
    let file = answers_file(name);
    let path = dir().join(&file);
    if update {
        match answers {
            Some(answers) => std::fs::write(&path, written(answers)).map_err(|e| e.to_string())?,
            None if path.exists() => std::fs::remove_file(&path).map_err(|e| e.to_string())?,
            None => {}
        }
        return Ok(());
    }
    let expected = match std::fs::read_to_string(&path) {
        Ok(text) => {
            Some(serde_json::from_str::<serde_json::Value>(&text).map_err(|e| format!("{file}: {e}"))?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{file}: {e}")),
    };
    match (answers, expected) {
        (None, None) => Ok(()),
        (Some(answers), Some(expected)) if *answers == expected => Ok(()),
        (Some(answers), Some(_)) => {
            Err(format!("its answers are not those of {file}. It answered:\n{}", written(answers)))
        }
        (Some(_), None) => Err(format!("it returns answers and there is no {file}")),
        (None, Some(_)) => Err(format!("it returns nothing and there is a {file}")),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_script_passes() {
    let update = std::env::var_os("NEOC_UPDATE_ANSWERS").is_some();
    let scripts = scripts();
    assert!(!scripts.is_empty(), "no tests/scripts/*.luau was found");
    let mut failures = Vec::new();
    for (name, code) in &scripts {
        let outcome = match run(code).await {
            Ok(answers) => check_answers(name, answers.as_ref(), update),
            Err(error) => Err(error),
        };
        match outcome {
            Ok(()) => println!("ok      {name}"),
            Err(why) => {
                println!("FAILED  {name}");
                failures.push(format!("{name}.luau: {why}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} script(s) failed:\n{}", failures.len(), failures.join("\n"));
}

/// An answers file with no script is one nobody checks.
#[test]
fn every_answers_file_has_its_script() {
    let orphans: Vec<String> = std::fs::read_dir(dir())
        .expect("the scripts directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|file| {
            file.strip_suffix(".json").is_some_and(|name| {
                let module = FAMILIES.iter().find_map(|family| name.strip_suffix(&format!(".{family}")));
                !dir().join(format!("{}.luau", module.unwrap_or(name))).is_file()
            })
        })
        .collect();
    assert!(orphans.is_empty(), "answers with no script: {}", orphans.join(", "));
}
