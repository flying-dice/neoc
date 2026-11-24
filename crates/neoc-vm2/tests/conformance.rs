//! The same Luau scripts, run against neoc-vm (the reference) and neoc-vm2.
//!
//! `conformance/<module>.luau` runs on both VMs and returns plain values; the two answers must be equal.
//! `conformance/<module>.vm2.luau` runs on neoc-vm2 only and asserts what it deliberately does differently; its
//! first line says what changed (`-- intentional: ...`), and the harness prints those so the list of intended
//! differences is always at hand (`cargo test -p neoc-vm2 --test conformance -- --nocapture`).

use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance")
}

fn scripts(suffix: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = std::fs::read_dir(dir())
        .expect("the conformance directory")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(suffix)?;
            // `base64.luau` is not `base64.vm2.luau`
            (!name.ends_with(".vm2"))
                .then(|| (name.to_owned(), std::fs::read_to_string(entry.path()).expect("a script")))
        })
        .collect();
    found.sort();
    found
}

async fn on_vm1(code: &str) -> Result<serde_json::Value, String> {
    use neoc_vm::mlua::LuaSerdeExt;
    let lua = neoc_vm::lua::sandbox::make_engine().map_err(|e| e.to_string())?;
    let value: neoc_vm::mlua::Value = lua.load(code).eval_async().await.map_err(|e| e.to_string())?;
    lua.from_value(value).map_err(|e| e.to_string())
}

async fn on_vm2(code: &str) -> Result<serde_json::Value, String> {
    use neoc_vm2::mlua::LuaSerdeExt;
    let lua = neoc_vm2::make_engine().map_err(|e| e.to_string())?;
    let value: neoc_vm2::mlua::Value = lua.load(code).eval_async().await.map_err(|e| e.to_string())?;
    lua.from_value(value).map_err(|e| e.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_vms_answer_the_same() {
    let common = scripts(".luau");
    assert!(common.len() >= 2, "a script per ported module");
    let mut differences = Vec::new();
    for (name, code) in &common {
        let (one, two) = (on_vm1(code).await, on_vm2(code).await);
        match (&one, &two) {
            (Ok(a), Ok(b)) if a == b => println!("same      {name}"),
            _ => {
                println!("DIFFERENT {name}");
                differences.push(format!("{name}:\n  neoc-vm : {one:?}\n  neoc-vm2: {two:?}"));
            }
        }
    }
    assert!(differences.is_empty(), "the VMs disagree:\n{}", differences.join("\n"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn neoc_vm2_keeps_to_what_it_deliberately_does_differently() {
    let scripts = scripts(".vm2.luau");
    assert!(!scripts.is_empty(), "the intended differences are asserted");
    for (name, code) in &scripts {
        // `--!strict` (for luau-lsp) may come before it
        let first = code.lines().find(|line| *line != "--!strict").unwrap_or_default();
        let reason = first
            .strip_prefix("-- intentional: ")
            .unwrap_or_else(|| panic!("{name}.vm2.luau starts with `-- intentional: <what changed>`"));
        println!("intentional  {name}: {reason}");
        if let Err(e) = on_vm2(code).await {
            panic!("{name}.vm2.luau failed:\n{e}");
        }
    }
}
