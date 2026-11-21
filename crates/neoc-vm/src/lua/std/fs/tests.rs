//! `std:fs` through the Lua VM: the conventions every function shares (the `(nil, err)` shape, the error kinds,
//! options that reject typos) and each function used the way a script uses it. The operations themselves are tested
//! one level down, in `ops.rs`.

use mlua::Lua;

use crate::lua::sandbox::make_engine;

/// Runs `body` as a script with `dir` (a fresh temp directory) and the helpers below in scope.
async fn run(body: &str) {
    let dir = tempfile::tempdir().unwrap();
    let lua = make_engine().unwrap();
    lua.globals()
        .set("dir", dir.path().to_str().unwrap())
        .unwrap();
    run_in(&lua, body).await;
}

async fn run_in(lua: &Lua, body: &str) {
    const PRELUDE: &str = r#"
        local fs, path = require("std:fs"), require("std:path")
        local function p(name) return path.join(dir, name) end
        local function check(cond, message) if not cond then error(message or "check failed", 3) end end
        -- the call must have failed with this kind: `fails("not_found", fs.read(p("x")))`
        local function fails(kind, value, err)
          check(value == nil, "expected a failure, got " .. tostring(value))
          check(type(err) == "string", "expected an error string")
          check(fs.error_kind(err) == kind, "expected " .. kind .. ", got " .. err)
        end
        local function ok(value, err) check(err == nil, tostring(err)) return value end
        local function names(entries)
          local out = {}
          for i, e in ipairs(entries) do out[i] = e:file_name() end
          return table.concat(out, ",")
        end
    "#;
    if let Err(e) = lua.load(format!("{PRELUDE}\n{body}")).exec_async().await {
        panic!("{e}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_failure_is_nil_then_an_error_with_a_stable_kind() {
    run(r#"
        local _, err = fs.read(p("nope"))
        check(err:find("^fs%.read: not_found: "), err)
        check(err:find(p("nope"), 1, true), "the error names the path")
        fails("not_found", fs.read(p("nope")))
        fails("not_found", fs.metadata(p("nope")))
        fails("not_found", fs.symlink_metadata(p("nope")))
        fails("not_found", fs.read_dir(p("nope")))
        fails("not_found", fs.walk(p("nope")))
        fails("not_found", fs.copy(p("nope"), p("x")))
        fails("not_found", fs.copy_dir(p("nope"), p("x")))
        fails("not_found", fs.rename(p("nope"), p("x")))
        fails("not_found", fs.move(p("nope"), p("x")))
        fails("not_found", fs.remove_file(p("nope")))
        fails("not_found", fs.remove_dir(p("nope")))
        fails("not_found", fs.remove_dir_all(p("nope")))
        fails("not_found", fs.canonicalize(p("nope")))
        fails("not_found", fs.set_permissions(p("nope"), { readonly = true }))
        fails("not_found", fs.create_dir(p("a/b")))
        fails("not_found", fs.open({ path = p("nope"), mode = "r" }))
        check(fs.error_kind("fs.read: not_found: x") == "not_found")
        check(fs.error_kind("something else") == nil)
        check(fs.error_kind("fs.read: bogus_kind: x") == nil)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn existing_functions_follow_the_contract_too() {
    run(r#"
        ok(fs.write(p("f"), "hello"))
        check(ok(fs.read(p("f"))) == "hello")
        check(ok(fs.create_dir(p("d"))) == true)
        fails("already_exists", fs.create_dir(p("d")))
        check(ok(fs.create_dir_all(p("d"))) == true)
        check(ok(fs.create_dir_all(p("a/b/c"))) == true)
        fails("not_a_directory", fs.create_dir_all(p("f/x")))
        fails("is_directory", fs.read(p("d")))
        fails("invalid_input", fs.open({ path = p("f"), mode = "x" }))
        fails("invalid_input", fs.open({ path = p("f") }))
        local w = ok(fs.open({ path = p("w"), mode = "w" }))
        check(w ~= nil)
        check(ok(fs.read(p("w"))) == "")
        check(fs.open({ path = p("f"), mode = "r" }) ~= nil)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exists_and_metadata_describe_paths() {
    run(r#"
        check(ok(fs.exists(dir)) == true)
        check(ok(fs.exists(p("nope"))) == false)
        ok(fs.write(p("f"), "12345"))
        local m = ok(fs.metadata(p("f")))
        check(m:kind() == "file" and m:is_file() and not m:is_dir() and not m:is_symlink())
        check(m:len() == 5)
        check(m:modified() ~= nil and m:modified() > 1e9, "modified is a recent epoch time")
        check(type(m:readonly()) == "boolean")
        local d = ok(fs.metadata(dir))
        check(d:kind() == "dir" and d:is_dir() and not d:is_file())
        check(m:created() == nil or m:created() > 1e9)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_dir_lists_sorted_entries_with_kinds_and_paths() {
    run(r#"
        ok(fs.write(p("b.txt"), "b"))
        ok(fs.write(p("a.txt"), "a"))
        ok(fs.create_dir(p("sub")))
        ok(fs.write(p("sub/inner"), "i"))
        local entries = ok(fs.read_dir(dir))
        check(#entries == 3)
        check(names(entries) == "a.txt,b.txt,sub", names(entries))
        check(entries[1]:kind() == "file" and entries[1]:is_file() and entries[1]:depth() == 1)
        check(entries[3]:kind() == "dir" and entries[3]:is_dir())
        check(entries[3]:path() == p("sub"))
        check(names(ok(fs.read_dir(entries[3]:path()))) == "inner")
        check(#ok(fs.read_dir(p("sub/../sub"))) == 1)
        fails("not_a_directory", fs.read_dir(p("a.txt")))
        ok(fs.create_dir(p("empty")))
        check(#ok(fs.read_dir(p("empty"))) == 0)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn walk_lists_a_tree_depth_first_and_honours_max_depth() {
    run(r#"
        ok(fs.create_dir_all(p("t/sub/deeper")))
        ok(fs.write(p("t/b"), "b"))
        ok(fs.write(p("t/a"), "a"))
        ok(fs.write(p("t/sub/c"), "c"))
        ok(fs.write(p("t/sub/deeper/d"), "d"))
        local all = ok(fs.walk(p("t")))
        check(names(all) == "a,b,sub,c,deeper,d", names(all))
        check(all[1]:depth() == 1 and all[4]:depth() == 2 and all[6]:depth() == 3)
        check(all[6]:path() == p("t/sub/deeper/d"))
        check(#ok(fs.walk(p("t"), { max_depth = 1 })) == 3)
        check(#ok(fs.walk(p("t"), { max_depth = 0 })) == 0)
        check(#ok(fs.walk(p("t"), { max_depth = 2^53 })) == 6)
        check(#ok(fs.walk(p("t"), { max_depth = 1e300 })) == 6, "a huge depth is simply unbounded")
        check(#ok(fs.walk(p("t"), {})) == 6 and #ok(fs.walk(p("t"), nil)) == 6)
        fails("invalid_input", fs.walk(p("t"), { max_depth = -1 }))
        fails("invalid_input", fs.walk(p("t"), { max_depth = 0/0 }))
        fails("invalid_input", fs.walk(p("t"), { max_depth = 1.5 }))
        fails("invalid_input", fs.walk(p("t"), { max_depth = 1/0 }))
        fails("invalid_input", fs.walk(p("t"), { max_depth = "2" }))
        fails("invalid_input", fs.walk(p("t"), { depth = 2 }))
        fails("not_a_directory", fs.walk(p("t/a")))
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_replaces_a_file_like_std_and_refuses_what_it_cannot_copy() {
    run(r#"
        ok(fs.write(p("a"), "one"))
        check(ok(fs.copy(p("a"), p("b"))) == 3, "copy returns the bytes written")
        check(ok(fs.read(p("b"))) == "one")
        ok(fs.write(p("a"), "two!"))
        check(ok(fs.copy(p("a"), p("b"))) == 4, "an existing file is replaced")
        check(ok(fs.read(p("b"))) == "two!")
        ok(fs.create_dir(p("d")))
        fails("is_directory", fs.copy(p("a"), p("d")))
        fails("is_directory", fs.copy(p("d"), p("e")))
        fails("invalid_input", fs.copy(p("a"), p("a")))
        check(ok(fs.read(p("a"))) == "two!", "copying onto itself does not empty it")
        fails("not_found", fs.copy(p("a"), p("no/such/b")))
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_dir_copies_a_tree_into_a_new_path_only() {
    run(r#"
        ok(fs.create_dir_all(p("src/sub/deeper")))
        ok(fs.write(p("src/a"), "a"))
        ok(fs.write(p("src/sub/b"), "bb"))
        ok(fs.write(p("src/sub/deeper/c"), "ccc"))
        fails("not_a_directory", fs.copy_dir(p("src/a"), p("x")))
        check(ok(fs.copy_dir(p("src"), p("dst"))) == 6)
        check(ok(fs.read(p("dst/sub/deeper/c"))) == "ccc")
        check(ok(fs.read(p("src/a"))) == "a", "the source is left alone")
        fails("already_exists", fs.copy_dir(p("src"), p("dst")))
        fails("invalid_input", fs.copy_dir(p("src"), p("src/sub/inside")))
        check(fs.exists(p("src/sub/inside")) == false)
        fails("not_found", fs.copy_dir(p("src"), p("no/such/dst")))
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_and_move_have_the_same_destination_rules() {
    run(r#"
        for _, op in ipairs { fs.rename, fs.move } do
          ok(fs.write(p("a"), "a"))
          ok(op(p("a"), p("b")))
          check(fs.exists(p("a")) == false and ok(fs.read(p("b"))) == "a")
          ok(fs.write(p("a"), "new"))
          ok(op(p("a"), p("b")))
          check(ok(fs.read(p("b"))) == "new", "an existing file is replaced")
          ok(fs.create_dir(p("full")))
          ok(fs.write(p("full/x"), "x"))
          ok(fs.write(p("a"), "a"))
          fails("is_directory", op(p("a"), p("full")))
          check(ok(fs.read(p("a"))) == "a", "a refused move changes nothing")
          ok(fs.remove_file(p("a")))
          ok(fs.remove_file(p("b")))
          ok(fs.remove_dir_all(p("full")))
        end
        ok(fs.create_dir_all(p("tree/sub")))
        ok(fs.write(p("tree/sub/f"), "f"))
        ok(fs.move(p("tree"), p("moved")))
        check(fs.exists(p("tree")) == false and ok(fs.read(p("moved/sub/f"))) == "f")
        ok(fs.rename(p("moved"), p("renamed")))
        check(ok(fs.read(p("renamed/sub/f"))) == "f")
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remove_functions_have_explicit_scopes_and_flags() {
    run(r#"
        ok(fs.write(p("f"), "x"))
        check(ok(fs.remove_file(p("f"))) == true)
        fails("not_found", fs.remove_file(p("f")))
        check(ok(fs.remove_file(p("f"), { missing_ok = true })) == false)
        ok(fs.create_dir_all(p("tree/sub")))
        ok(fs.write(p("tree/sub/f"), "x"))
        fails("is_directory", fs.remove_file(p("tree")))
        fails("not_empty", fs.remove_dir(p("tree")))
        check(fs.exists(p("tree/sub/f")) == true)
        check(ok(fs.remove_dir_all(p("tree"))) == true)
        check(fs.exists(p("tree")) == false)
        check(ok(fs.remove_dir_all(p("tree"), { missing_ok = true })) == false)
        ok(fs.create_dir(p("empty")))
        check(ok(fs.remove_dir(p("empty"))) == true)
        check(ok(fs.remove_dir(p("empty"), { missing_ok = true })) == false)
        ok(fs.write(p("f"), "x"))
        fails("not_a_directory", fs.remove_dir(p("f")))
        fails("not_a_directory", fs.remove_dir_all(p("f")))
        fails("invalid_input", fs.remove_dir_all("", { missing_ok = true }))
        fails("invalid_input", fs.remove_dir_all("/"))
        fails("invalid_input", fs.remove_dir_all("."))
        fails("invalid_input", fs.remove_dir_all(".."))
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn options_reject_typos_and_wrong_types_instead_of_ignoring_them() {
    run(r#"
        ok(fs.write(p("a"), "a"))
        fails("invalid_input", fs.remove_file(p("a"), { missing = true }))
        fails("invalid_input", fs.remove_file(p("a"), { missing_ok = "yes" }))
        fails("invalid_input", fs.remove_file(p("a"), { missing_ok = 1 }))
        fails("invalid_input", fs.remove_file(p("a"), { true }))
        fails("invalid_input", fs.remove_dir(p("a"), { recursive = true }))
        fails("invalid_input", fs.remove_dir_all(p("a"), { recursive = true }))
        fails("invalid_input", fs.set_permissions(p("a"), {}))
        fails("invalid_input", fs.set_permissions(p("a"), { mode = 420, readonly = false }))
        fails("invalid_input", fs.set_permissions(p("a"), { mode = 4096 }))
        fails("invalid_input", fs.set_permissions(p("a"), { mode = -1 }))
        fails("invalid_input", fs.set_permissions(p("a"), { mode = 2^63 }))
        fails("invalid_input", fs.set_permissions(p("a"), { mode = 1e300 }))
        fails("invalid_input", fs.set_permissions(p("a"), { readonly = "no" }))
        fails("invalid_input", fs.set_permissions(p("a"), { writable = true }))
        check(fs.exists(p("a")) == true, "a rejected call does nothing")
        check(ok(fs.remove_file(p("a"), {})) == true, "an empty table is the defaults")
        ok(fs.write(p("a"), "a"))
        check(ok(fs.remove_file(p("a"), nil)) == true, "nil is the defaults")
        ok(fs.write(p("a"), "a"))
        local message = select(2, fs.remove_file(p("a"), { missing = true }))
        check(message:find("missing", 1, true) and message:find("missing_ok", 1, true), message)
        check(fs.exists(p("a")) == true)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_argument_type_is_a_lua_error_never_a_panic() {
    let lua = make_engine().unwrap();
    for call in [
        "fs.read({})",
        "fs.read()",
        "fs.copy('a')",
        "fs.copy_dir('a', {})",
        "fs.walk('a', 5)",
        "fs.remove_dir_all('a', 'recursive')",
        "fs.read_dir(function() end)",
        "fs.metadata(nil)",
        "fs.set_permissions('a')",
        "fs.error_kind({})",
    ] {
        let outcome: mlua::Result<mlua::MultiValue> = lua
            .load(format!("local fs = require('std:fs'); return {call}"))
            .eval_async()
            .await;
        assert!(outcome.is_err(), "{call} should raise a Lua error");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hostile_paths_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let lua = make_engine().unwrap();
    lua.globals()
        .set("dir", dir.path().to_str().unwrap())
        .unwrap();
    lua.load(
        r#"
        local fs = require("std:fs")
        local long = ("x"):rep(5000)
        for _, bad in ipairs { "", "a\0b", "\0", long, ("a/"):rep(3000) } do
          local label = tostring(#tostring(bad))
          local v, err = fs.read(bad)
          assert(v == nil and fs.error_kind(err), label .. ": " .. tostring(err))
          assert(fs.metadata(bad) == nil)
          assert(fs.exists(bad) == nil or fs.exists(bad) == false)
          assert(fs.read_dir(bad) == nil)
          assert(fs.walk(bad) == nil)
          assert(fs.remove_file(bad) == nil)
          assert(fs.remove_dir(bad) == nil)
          assert(fs.remove_dir_all(bad) == nil)
          assert(fs.copy(bad, bad) == nil)
          assert(fs.copy_dir(bad, bad) == nil)
          assert(fs.move(bad, bad) == nil)
          assert(fs.rename(bad, bad) == nil)
          assert(fs.symlink(bad, bad) == nil)
          assert(fs.read_link(bad) == nil)
          assert(fs.canonicalize(bad) == nil)
          assert(fs.set_permissions(bad, { readonly = true }) == nil)
        end
        local _, err = fs.write("a\0b", "x")
        assert(fs.error_kind(err) == "invalid_input" and err:find("NUL", 1, true), err)
        "#,
    )
    .exec_async()
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_symlink_loop_cannot_hang_a_walk_a_copy_or_a_remove() {
    run(r#"
        ok(fs.create_dir_all(p("t/sub")))
        ok(fs.write(p("t/sub/f"), "f"))
        ok(fs.symlink(dir, p("t/loop")))
        ok(fs.symlink("sub/..", p("t/sub/self")))
        ok(fs.symlink("loop2", p("l1")))
        ok(fs.symlink("l1", p("loop2")))
        check(#ok(fs.walk(p("t"))) == 4, "the links are listed, not entered")
        check(ok(fs.copy_dir(p("t"), p("copy"))) == 1)
        check(ok(fs.symlink_metadata(p("copy/loop"))):is_symlink())
        check(ok(fs.remove_dir_all(p("copy"))) == true)
        check(ok(fs.remove_dir_all(p("t"))) == true)
        check(fs.exists(p("t")) == false)
        -- a link that loops on itself: following it fails, the link's own metadata does not
        check(select(1, fs.metadata(p("l1"))) == nil and select(2, fs.metadata(p("l1"))) ~= nil)
        check(ok(fs.symlink_metadata(p("l1"))):is_symlink())
        check(select(1, fs.copy(p("l1"), p("x"))) == nil)
        check(ok(fs.remove_file(p("l1"))) == true)
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn symlinks_are_reported_not_followed_and_removed_as_links() {
    run(r#"
        ok(fs.create_dir(p("real")))
        ok(fs.write(p("real/keep"), "k"))
        ok(fs.write(p("file"), "f"))
        ok(fs.symlink("real", p("dirlink")))
        ok(fs.symlink("file", p("filelink")))
        ok(fs.symlink("gone", p("dangling")))
        fails("already_exists", fs.symlink("real", p("dirlink")))
        check(ok(fs.read_link(p("dirlink"))) == "real")
        fails("invalid_input", fs.read_link(p("file")))
        check(ok(fs.symlink_metadata(p("dirlink"))):kind() == "symlink")
        check(ok(fs.symlink_metadata(p("dirlink"))):is_symlink())
        check(ok(fs.metadata(p("dirlink"))):kind() == "dir", "metadata follows")
        check(ok(fs.metadata(p("filelink"))):len() == 1)
        check(ok(fs.symlink_metadata(p("dangling"))):kind() == "symlink")
        fails("not_found", fs.metadata(p("dangling")))
        check(ok(fs.exists(p("dangling"))) == false, "a dangling link does not exist")
        check(path.file_name(ok(fs.canonicalize(p("dirlink")))) == "real")
        local listed = {}
        for _, e in ipairs(ok(fs.read_dir(dir))) do listed[e:file_name()] = e:kind() end
        check(listed.dirlink == "symlink" and listed.filelink == "symlink" and listed.real == "dir", "read_dir does not resolve links")
        -- a recursive remove deletes the links and never what they point at
        ok(fs.create_dir(p("victim")))
        ok(fs.write(p("victim/precious"), "p"))
        ok(fs.create_dir(p("tree")))
        ok(fs.symlink(p("victim"), p("tree/to_victim")))
        ok(fs.symlink(dir, p("tree/loop")))
        check(ok(fs.remove_dir_all(p("tree"))) == true)
        check(fs.exists(p("tree")) == false)
        check(ok(fs.read(p("victim/precious"))) == "p")
        -- the directory removals do not go through a link, however it is spelled
        fails("not_a_directory", fs.remove_dir(p("dirlink")))
        fails("not_a_directory", fs.remove_dir_all(p("dirlink")))
        fails("not_a_directory", fs.remove_dir_all(p("dirlink") .. "/"))
        fails("not_a_directory", fs.remove_dir_all(p("dirlink") .. "/."))
        check(ok(fs.read(p("real/keep"))) == "k")
        check(ok(fs.remove_file(p("dirlink"))) == true)
        check(ok(fs.read(p("real/keep"))) == "k")
        -- a recursive copy recreates links and does not follow the loop
        ok(fs.create_dir(p("src")))
        ok(fs.write(p("src/f"), "f"))
        ok(fs.symlink(dir, p("src/loop")))
        ok(fs.copy_dir(p("src"), p("dst")))
        check(ok(fs.symlink_metadata(p("dst/loop"))):is_symlink())
        check(ok(fs.read_link(p("dst/loop"))) == dir)
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permissions_are_read_and_written_as_unix_bits() {
    run(r#"
        ok(fs.write(p("f"), "x"))
        local m = ok(fs.metadata(p("f")))
        check(type(m:mode()) == "number" and m:mode() > 0 and m:mode() <= 4095, "mode is the permission bits")
        check(ok(fs.set_permissions(p("f"), { mode = 493 })) == true)
        check(ok(fs.metadata(p("f"))):mode() == 493)
        ok(fs.set_permissions(p("f"), { readonly = true }))
        local r = ok(fs.metadata(p("f")))
        check(r:readonly() == true and r:mode() == 365, "readonly clears the write bits")
        ok(fs.set_permissions(p("f"), { readonly = false }))
        local w = ok(fs.metadata(p("f")))
        check(w:readonly() == false and w:mode() == 493, "and gives the owner write access back")
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_that_are_not_utf8_round_trip_through_read_dir() {
    use std::os::unix::ffi::OsStrExt;
    // some filesystems (APFS on macOS) refuse a name that is not UTF-8: nothing to round-trip there
    let probe = tempfile::tempdir().unwrap();
    if std::fs::write(
        probe
            .path()
            .join(std::ffi::OsStr::from_bytes(b"caf\xe9\xff")),
        "",
    )
    .is_err()
    {
        return;
    }
    run(r#"
        local odd = p("caf\233\255")
        ok(fs.write(odd, "odd"))
        local entries = ok(fs.read_dir(dir))
        check(#entries == 1 and entries[1]:file_name() == "caf\233\255")
        check(ok(fs.read(entries[1]:path())) == "odd")
        check(#ok(fs.walk(dir)) == 1)
        check(ok(fs.copy(entries[1]:path(), p("copy\255"))) == 3)
        check(ok(fs.remove_file(entries[1]:path())) == true)
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_denied_is_reported_as_such_and_never_hidden() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("f"), "x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&locked).is_ok(); // running as root ignores the mode
    let lua = make_engine().unwrap();
    lua.globals()
        .set("dir", dir.path().to_str().unwrap())
        .unwrap();
    if !readable {
        run_in(
            &lua,
            r#"
            fails("permission_denied", fs.read_dir(p("locked")))
            fails("permission_denied", fs.walk(p("locked")))
            fails("permission_denied", fs.read(p("locked/f")))
            fails("permission_denied", fs.exists(p("locked/f")))
            fails("permission_denied", fs.remove_dir_all(p("locked")))
            "#,
        )
        .await;
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_dropped_while_its_job_runs_cancels_the_job() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    let flag = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = mpsc::channel();
    let seen = Arc::clone(&flag);
    let future = super::blocking(move |cancel| {
        started_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !cancel.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        seen.store(cancel.load(Ordering::Relaxed), Ordering::SeqCst);
        Ok(())
    });
    let handle = tokio::spawn(future);
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
        .await
        .unwrap();
    handle.abort();
    let _ = handle.await;
    for _ in 0..500 {
        if flag.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the job never saw its cancel flag");
}

/// The long calls wait on the blocking pool, so a script's other work goes on meanwhile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_long_calls_yield_like_any_wait() {
    run(r#"
        local task = require("std:task")
        ok(fs.create_dir_all(p("t/sub")))
        ok(fs.write(p("t/sub/f"), "f"))
        local t = task.spawn(function() return ok(fs.copy_dir(p("t"), p("u"))) end)
        check(t:await() == 1)
        check(#ok(fs.walk(p("u"))) == 2)
        check(ok(fs.remove_dir_all(p("u"))) == true)
        -- Luau refuses a yield inside a comparator: the call fails, it does not hang or panic
        local sorted = pcall(table.sort, { 1, 2, 3 }, function(a, b) fs.walk(dir) return a < b end)
        check(sorted == false)
    "#)
    .await;
}
