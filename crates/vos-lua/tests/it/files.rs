#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! `app.files` driven from Luau apps against the real disk adapter (`DiskFiles`) in a temporary directory: the
//! fixture app's workflow (also in a VM that refuses `std:fs`), every escape form, two apps that do not see each
//! other's files, and read-only calls (and the tasks they start).

use crate::common;

use common::{agent_ctx, db, id, package};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use tempfile::TempDir;
use vos_application::FileStore;
use vos_lua::{AppVm, DiskFiles, VmConfig};

const FIXTURE: &str = include_str!("../fixtures/files/app.luau");

/// Load `src` as the app `name` with its files under `root`, refusing the library modules in `refused`.
async fn load_in(root: &Path, name: &str, src: &str, refused: &[&str]) -> AppVm {
    let store = DiskFiles::new(Some(root.to_path_buf()));
    let config = VmConfig {
        files: store.files(&id(name)),
        refused_modules: refused.iter().map(|m| (*m).to_owned()).collect(),
        ..common::config()
    };
    AppVm::load(&id(name), package(src, &[]), db(), config).await.unwrap()
}

async fn tool(vm: &AppVm, name: &str) -> Value {
    common::serve(vm, name, json!({}), &agent_ctx(0)).await.unwrap()
}

fn check_workflow(out: &Value) {
    let ok = |step: &str| assert_eq!(out[step]["ok"], true, "{step}: {out}");
    let failed = |step: &str, kind: &str| {
        assert_eq!(out[step]["ok"], false, "{step}: {out}");
        assert_eq!(out[step]["kind"], kind, "{step}: {out}");
    };
    for step in
        ["create_dir", "createAgain", "write", "overwrite", "move", "copyFile", "copyTree", "removeTree"]
    {
        ok(step);
    }
    failed("writeNoParent", "not_found");
    failed("readMissing", "not_found");
    failed("readDir", "is_directory");
    failed("oldGone", "not_found");
    failed("copyOverTree", "already_exists");
    failed("removeNotEmpty", "not_empty");
    failed("removeMissing", "not_found");
    assert_eq!(out["read"]["value"], "buy milk\nwalk the dog", "a write replaces the whole file: {out}");
    assert_eq!(out["readMoved"]["value"], "buy milk\nwalk the dog");
    assert_eq!(
        out["list"]["value"],
        json!([{ "name": "todo.txt", "kind": "file", "size": 21, "modified": out["info"]["value"]["modified"] }])
    );
    let info = &out["info"]["value"];
    assert_eq!(
        (&info["name"], &info["kind"], &info["size"]),
        (&json!("todo.txt"), &json!("file"), &json!(21))
    );
    assert!(info["modified"].as_f64().unwrap() > 1.7e9, "seconds since the epoch: {info}");
    assert_eq!(out["infoDir"]["value"]["kind"], "dir");
    let names = |v: &Value| {
        v.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
    };
    assert_eq!(names(&out["listBackup"]["value"]), ["copy.txt", "done.txt"], "the tree was copied whole");
    assert_eq!(names(&out["root"]["value"]), ["notes"], "the backup is gone: {out}");
    assert_eq!(out["cacheRead"]["value"], 3, "binary-safe, in its own location");
    ok("tempEmpty");
}

#[tokio::test]
async fn the_fixture_app_completes_its_file_workflow_through_app_files_alone() {
    let root = TempDir::new().unwrap();
    let vm = load_in(root.path(), "files-demo", FIXTURE, &[]).await;
    check_workflow(&tool(&vm, "workflow").await);
    // the files sit under the app's own directory, one per location
    let app_dir = root.path().join("files-demo.files");
    let done = std::fs::read_to_string(app_dir.join("documents/notes/2026/done.txt")).unwrap();
    assert_eq!(done, "buy milk\nwalk the dog");
    assert_eq!(std::fs::read(app_dir.join("cache/thumb.bin")).unwrap(), [0, 1, 2]);
    let leftovers: Vec<_> = std::fs::read_dir(app_dir.join("documents/notes/2026"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(leftovers, ["done.txt"], "no temporary file is left behind");
}

#[tokio::test]
async fn the_workflow_works_for_an_app_that_is_refused_std_fs() {
    let root = TempDir::new().unwrap();
    let src = format!(
        "{FIXTURE}\nRPC.on_request(\"fs\", function() local ok, err = pcall(require, \"std:fs\") return {{ ok = ok, err = tostring(err) }} end)"
    );
    let vm = load_in(root.path(), "files-demo", &src, &["std:fs"]).await;
    let fs = tool(&vm, "fs").await;
    assert_eq!(fs["ok"], false, "the app cannot require std:fs: {fs}");
    assert!(fs["err"].as_str().unwrap().contains("std:fs"), "{fs}");
    check_workflow(&tool(&vm, "workflow").await);
}

/// Every escape form, on every operation it applies to, is `invalid_input`, and nothing outside the location is
/// read, written or removed.
#[cfg(unix)]
#[tokio::test]
async fn no_path_leaves_the_apps_location() {
    let root = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let docs = root.path().join("probe.files/documents");
    std::fs::create_dir_all(docs.join("real")).unwrap();
    std::fs::write(docs.join("real/plain.txt"), "plain").unwrap();
    std::os::unix::fs::symlink(outside.path(), docs.join("link")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), docs.join("file_link")).unwrap();
    let outside_path = outside.path().join("secret.txt").display().to_string();

    let src = format!(
        r#"
        local docs = app.files.documents
        RPC.on_request("escapes", function()
          local out = {{}}
          local function try(label, value, err) out[label] = {{ ok = value ~= nil, kind = (err and err.kind), err = err }} end
          local long = string.rep("a", 256)
          try("absolute read", docs:read("{outside_path}"))
          try("absolute write", docs:write("/tmp/vos-escape.txt", "x"))
          try("absolute list", docs:list("/"))
          try("dotdot read", docs:read("../../probe.files/documents/real/plain.txt"))
          try("dotdot write", docs:write("real/../../escape.txt", "x"))
          try("dotdot copy", docs:copy("real/plain.txt", "../plain.txt"))
          try("dotdot move", docs:move("real/plain.txt", "../plain.txt"))
          try("dotdot remove", docs:remove("..", {{ recursive = true }}))
          try("nul read", docs:read("real/plain.txt\0.png"))
          try("nul write", docs:write("a\0b", "x"))
          try("long name", docs:write(long, "x"))
          try("long path", docs:read(string.rep("a/", 2100)))
          try("colon", docs:read("C:/Windows/win.ini"))
          try("backslash", docs:read("real\\plain.txt"))
          try("through link read", docs:read("link/secret.txt"))
          try("through link write", docs:write("link/new.txt", "x"))
          try("through link list", docs:list("link"))
          try("through link create", docs:create_dir("link/sub"))
          try("through link remove", docs:remove("link/secret.txt"))
          try("through link move out", docs:move("link/secret.txt", "stolen.txt"))
          try("through link move in", docs:move("real/plain.txt", "link/plain.txt"))
          try("through link copy out", docs:copy("link/secret.txt", "stolen.txt"))
          try("through link copy in", docs:copy("real/plain.txt", "link/plain.txt"))
          try("through link info", docs:info("link/secret.txt"))
          try("file link read", docs:read("file_link"))
          try("file link write", docs:write("file_link", "overwritten"))
          try("file link copy", docs:copy("file_link", "stolen.txt"))
          try("file link move", docs:move("file_link", "moved_link"))
          try("copy onto file link", docs:copy("real/plain.txt", "file_link"))
          out.link_info = docs:info("link")
          out.listing = docs:list()
          local removed, why = docs:remove("file_link")
          out.remove_link = {{ ok = removed, err = why }}
          return out
        end)
        "#
    );
    let vm = load_in(root.path(), "probe", &src, &[]).await;
    let out = tool(&vm, "escapes").await;
    let results = out.as_object().unwrap();
    for (label, r) in
        results.iter().filter(|(k, _)| !["link_info", "listing", "remove_link"].contains(&k.as_str()))
    {
        assert_eq!(r["ok"], false, "{label}: {r}");
        assert_eq!(r["kind"], "invalid_input", "{label}: {r}");
        let err = r["err"].as_str().unwrap();
        assert!(
            !err.contains(&root.path().display().to_string()),
            "{label}: no host path in the error: {err}"
        );
    }
    assert_eq!(results.len(), 32, "every case ran: {out}");
    // a link is described, not followed, and removing it removes the link only
    assert_eq!(out["link_info"]["kind"], "symlink");
    let kinds: Vec<(&str, &str)> = out["listing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["name"].as_str().unwrap(), e["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(kinds, [("file_link", "symlink"), ("link", "symlink"), ("real", "dir")]);
    assert_eq!(out["remove_link"]["ok"], true, "{out}");
    assert!(std::fs::symlink_metadata(docs.join("file_link")).is_err(), "the link is gone");
    // nothing outside was touched
    assert_eq!(std::fs::read_to_string(outside.path().join("secret.txt")).unwrap(), "secret");
    let outside_names: Vec<_> =
        std::fs::read_dir(outside.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(outside_names, ["secret.txt"]);
    assert!(!Path::new("/tmp/vos-escape.txt").exists());
    assert!(
        !root.path().join("probe.files/escape.txt").exists()
            && !root.path().join("probe.files/plain.txt").exists()
    );
    assert_eq!(std::fs::read_to_string(docs.join("real/plain.txt")).unwrap(), "plain");
}

const SHARED_NAME: &str = r#"
    local docs = app.files.documents
    hook("write", function(text) return docs:write("shared.txt", text) end)
    hook("read", function() local text, err = docs:read("shared.txt") return { text = text, kind = (err and err.kind) } end)
    hook("list", function() return #assert(docs:list()) end)
"#;

#[tokio::test]
async fn two_apps_never_see_each_others_files_through_the_framework() {
    let root = TempDir::new().unwrap();
    let a = load_in(root.path(), "app-a", SHARED_NAME, &[]).await;
    let b = load_in(root.path(), "app-b", SHARED_NAME, &[]).await;
    assert_eq!(a.emit("write", json!("from a")).await.unwrap(), json!(true));
    assert_eq!(b.emit("list", json!(null)).await.unwrap(), json!(0), "b's documents are empty");
    assert_eq!(b.emit("read", json!(null)).await.unwrap()["kind"], "not_found");
    assert_eq!(b.emit("write", json!("from b")).await.unwrap(), json!(true));
    assert_eq!(a.emit("read", json!(null)).await.unwrap()["text"], "from a", "b's write landed in b's files");
    assert_eq!(b.emit("read", json!(null)).await.unwrap()["text"], "from b");
    assert!(root.path().join("app-a.files/documents/shared.txt").exists());
    assert!(root.path().join("app-b.files/documents/shared.txt").exists());
}

#[tokio::test]
async fn a_read_only_tool_reads_files_and_cannot_write_them_nor_can_the_tasks_it_starts() {
    let root = TempDir::new().unwrap();
    let src = r#"
        local task = require("std:task")
        local docs = app.files.documents
        local seen = {}
        local function refused(f) local ok, e = pcall(f) return { ok = ok, err = tostring(e) } end
        RPC.on_request("seed", function()
          assert(docs:create_dir("d"))
          return assert(docs:write("d/a.txt", "kept"))
        end)
        RPC.on_request("ro", function()
          task.spawn(function()
            task.wait(0.01)
            seen.task = refused(function() return docs:write("from-task.txt", "x") end)
          end)
          return {
            read = docs:read("d/a.txt"),
            listed = #assert(docs:list("d")),
            size = assert(docs:info("d/a.txt")).size,
            write = refused(function() return docs:write("d/a.txt", "changed") end),
            create_dir = refused(function() return docs:create_dir("e") end),
            remove = refused(function() return docs:remove("d", { recursive = true }) end),
            move = refused(function() return docs:move("d/a.txt", "b.txt") end),
            copy = refused(function() return docs:copy("d/a.txt", "b.txt") end),
            cache = refused(function() return app.files.cache:write("x", "x") end),
          }
        end, { read_only = true })
        RPC.on_request("seen", function() return seen end, { read_only = true })
    "#;
    let vm = load_in(root.path(), "probe", src, &[]).await;
    assert_eq!(tool(&vm, "seed").await, json!(true));
    let out = tool(&vm, "ro").await;
    assert_eq!((&out["read"], &out["listed"], &out["size"]), (&json!("kept"), &json!(1), &json!(4)), "{out}");
    for write in ["write", "create_dir", "remove", "move", "copy", "cache"] {
        assert_eq!(out[write]["ok"], false, "{write}: {out}");
        assert!(out[write]["err"].as_str().unwrap().contains("read-only"), "{write}: {out}");
    }
    let mut seen = tool(&vm, "seen").await;
    for _ in 0..200 {
        if seen.get("task").is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        seen = tool(&vm, "seen").await;
    }
    assert_eq!(seen["task"]["ok"], false, "the task inherits the read-only call: {seen}");
    assert!(seen["task"]["err"].as_str().unwrap().contains("read-only"), "{seen}");
    let docs = root.path().join("probe.files/documents");
    assert_eq!(std::fs::read_to_string(docs.join("d/a.txt")).unwrap(), "kept");
    assert!(!docs.join("from-task.txt").exists() && !docs.join("e").exists() && !docs.join("b.txt").exists());
    // a caller can require a read-only run of a writable tool too
    let err = common::serve_with(&vm, "seed", json!({}), &agent_ctx(0), true).await.unwrap_err().to_string();
    assert!(err.contains("read-only"), "{err}");
}

#[tokio::test]
async fn errors_are_values_with_a_stable_kind_and_misuse_raises() {
    let root = TempDir::new().unwrap();
    let src = r#"
        local docs = app.files.documents
        hook("probe", function()
          local _, missing = docs:read("nope.txt")
          local _, bad = docs:read("../x")
          local _, root_write = docs:write("", "x")
          return {
            missing = missing,
            missing_kind = missing and missing.kind,
            missing_text = tostring(missing),
            bad = bad,
            bad_kind = bad and bad.kind,
            root_kind = root_write and root_write.kind,
            -- `assert` raises the error itself, so what `pcall` catches is still the typed table
            raised_kind = select(2, pcall(function() return assert(docs:read("nope.txt")) end)).kind,
            bad_option = tostring(select(2, pcall(function() return docs:remove("x", { recrusive = true }) end))),
            bad_type = (pcall(function() return docs:read({} :: any) end)),
          }
        end)
    "#;
    let vm = load_in(root.path(), "probe", src, &[]).await;
    let out = vm.emit("probe", json!(null)).await.unwrap();
    // an error is a table: its kind, a message for a person (the path, then the wording), and the call
    let missing = &out["missing"];
    assert_eq!(
        (&missing["kind"], &missing["op"]),
        (&json!("not_found"), &json!("vos:app.Location:read")),
        "{out}"
    );
    let message = missing["message"].as_str().unwrap();
    assert!(message.starts_with("nope.txt: "), "{out}");
    assert_eq!(out["missing_text"], json!(format!("vos:app.Location:read: not_found: {message}")), "{out}");
    assert_eq!(
        (&out["missing_kind"], &out["bad_kind"], &out["root_kind"], &out["raised_kind"]),
        (&json!("not_found"), &json!("invalid_input"), &json!("invalid_input"), &json!("not_found"))
    );
    assert!(out["bad"]["message"].as_str().is_some_and(|m| !m.is_empty()), "{out}");
    assert!(out["bad_option"].as_str().unwrap().contains("recrusive"), "{out}");
    assert_eq!(out["bad_type"], false, "a table is not a path: {out}");
}

#[tokio::test]
async fn purging_removes_the_apps_files_and_a_host_without_a_data_directory_keeps_none() {
    let root = TempDir::new().unwrap();
    let vm = load_in(root.path(), "files-demo", FIXTURE, &[]).await;
    tool(&vm, "workflow").await;
    let store = DiskFiles::new(Some(root.path().to_path_buf()));
    assert!(root.path().join("files-demo.files").exists());
    store.purge(&id("files-demo")).await.unwrap();
    assert!(!root.path().join("files-demo.files").exists());
    store.purge(&id("files-demo")).await.unwrap();

    let none = DiskFiles::new(None);
    let config = VmConfig { files: none.files(&id("probe")), ..common::config() };
    let src = r#"hook("probe", function() local _, err = app.files.documents:list() return (err and err.kind) end)"#;
    let vm = AppVm::load(&id("probe"), package(src, &[]), db(), config).await.unwrap();
    assert_eq!(vm.emit("probe", json!(null)).await.unwrap(), json!("unsupported"));
    none.purge(&id("probe")).await.unwrap();
}

/// Where Luau cannot yield (a `string.gsub` callback, a `table.sort` comparator) a file call fails with the yield
/// error and does nothing: its job starts only once the call has really suspended.
#[tokio::test]
async fn a_file_call_where_luau_cannot_yield_fails_and_changes_nothing() {
    let root = TempDir::new().unwrap();
    let src = r#"
        local docs = app.files.documents
        hook("probe", function()
          assert(docs:write("kept.txt", "kept"))
          local function why(f) local ok, e = pcall(f) return (not ok) and tostring(e) or "ran" end
          return {
            gsub = why(function() string.gsub("a", "a", function() docs:write("gsub.txt", "x") end) end),
            sort = why(function() table.sort({ 2, 1 }, function(x, y) docs:remove("kept.txt") return x < y end) end),
            after = docs:read("kept.txt"),
          }
        end)
    "#;
    let vm = load_in(root.path(), "probe", src, &[]).await;
    let out = vm.emit("probe", json!(null)).await.unwrap();
    assert!(out["gsub"].as_str().unwrap().contains("yield"), "{out}");
    assert!(out["sort"].as_str().unwrap().contains("yield"), "{out}");
    assert_eq!(out["after"], "kept", "the call still works afterwards and the remove never ran: {out}");
    let docs = root.path().join("probe.files/documents");
    assert!(!docs.join("gsub.txt").exists(), "the refused write never ran");
    // give a job that was wrongly started the time to finish, then look again
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!docs.join("gsub.txt").exists() && docs.join("kept.txt").exists());
}

/// A write or a file copy into a directory the host cannot write fails with the app's path and the OS's words,
/// never a host path (`tempfile` names the host path in its own errors). Skipped as root, who can write anyway.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_write_or_copy_names_the_app_path_never_a_host_path() {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new().unwrap();
    let docs = root.path().join("probe.files/documents");
    let locked = docs.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(docs.join("src.txt"), "x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(locked.join("probe"), "").is_ok() {
        eprintln!("skipped: running as a user who can write a 0500 directory (root)");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let src = r#"
        local docs = app.files.documents
        hook("probe", function()
          local _, write = docs:write("locked/a.txt", "x")
          local _, copy = docs:copy("src.txt", "locked/b.txt")
          return { write = write, copy = copy }
        end)
    "#;
    let vm = load_in(root.path(), "probe", src, &[]).await;
    let out = vm.emit("probe", json!(null)).await.unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    let host = root.path().display().to_string();
    for (call, app_path) in [("write", "locked/a.txt"), ("copy", "locked/b.txt")] {
        let err = out[call].as_str().unwrap_or_else(|| panic!("{call} failed: {out}"));
        assert!(err.starts_with(&format!("files.{call}: permission_denied: {app_path}: ")), "{err}");
        assert!(!err.contains(&host) && !err.contains(".vos-tmp-") && !err.contains("at path"), "{err}");
    }
    assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 0, "nothing was left in the directory");
}
