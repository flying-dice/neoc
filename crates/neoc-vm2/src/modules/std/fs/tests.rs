//! `std:fs` through the Lua VM: the conventions every function shares (the `(nil, err)` shape, the typed error,
//! options that refuse typos) and each function used the way a script uses it. The operations themselves are tested
//! one level down, in `ops.rs`.
//!
//! Ported from neoc-vm's `std/fs/tests.rs`. The tests that drove a call from a `std:task` task are not here yet:
//! `std:task` is not ported.

use mlua::Lua;

use crate::make_engine;

/// Runs `body` as a script with `dir` (a fresh temp directory) and the helpers below in scope.
async fn run(body: &str) {
    let dir = tempfile::tempdir().unwrap();
    let lua = make_engine().unwrap();
    lua.globals().set("dir", dir.path().to_str().unwrap()).unwrap();
    run_in(&lua, body).await;
}

async fn run_in(lua: &Lua, body: &str) {
    const PRELUDE: &str = r#"
        local fs, path, io, time = require("std:fs"), require("std:path"), require("std:io"), require("std:time")
        local windows = require("std:env").family == "windows"
        local function p(name) return path.join(dir, name) end
        local function check(cond, message) if not cond then error(message or "check failed", 3) end end
        -- the call must have failed with this kind: `fails("not_found", fs.read(p("x")))`
        local function fails(kind, value, err)
          check(value == nil, "expected a failure, got " .. tostring(value))
          check(type(err) == "table", "expected an error")
          check(err.kind == kind, "expected " .. kind .. ", got " .. tostring(err))
        end
        -- the call must have been refused as a wrong argument: `refused("opts", fs.walk, dir, { depth = 2 })`
        local function refused(what, f, ...)
          local ok, err = pcall(f, ...)
          check(not ok, "expected the call to be refused")
          check(tostring(err):find(what, 1, true), tostring(err))
        end
        local function ok(value, err) check(err == nil, tostring(err)) return value end
        local function names(entries)
          local out = {}
          for i, e in ipairs(entries) do out[i] = e.file_name end
          return table.concat(out, ",")
        end
    "#;
    if let Err(e) = lua.load(format!("{PRELUDE}\n{body}")).exec_async().await {
        panic!("{e}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_failure_is_nil_then_a_typed_error() {
    run(r#"
        local value, err = fs.read(p("nope"))
        check(value == nil and err.kind == "not_found" and err.op == "std:fs.read")
        check(err.path == p("nope") and err.to == nil and type(err.reason) == "string")
        check(err.message == p("nope") .. ": " .. err.reason, err.message)
        check(tostring(err) == "std:fs.read: not_found: " .. err.message, tostring(err))
        fails("not_found", fs.read(p("nope")))
        fails("not_found", fs.metadata(p("nope")))
        fails("not_found", fs.symlink_metadata(p("nope")))
        fails("not_found", fs.read_dir(p("nope")))
        fails("not_found", fs.walk(p("nope")))
        fails("not_found", fs.copy(p("nope"), p("x")))
        fails("not_found", fs.copy_dir(p("nope"), p("x")))
        fails("not_found", fs.rename(p("nope"), p("x")))
        fails("not_found", fs.move_path(p("nope"), p("x")))
        fails("not_found", fs.remove_file(p("nope")))
        fails("not_found", fs.remove_dir(p("nope")))
        fails("not_found", fs.remove_dir_all(p("nope")))
        fails("not_found", fs.canonicalize(p("nope")))
        fails("not_found", fs.set_readonly(p("nope"), true))
        fails("not_found", fs.create_dir(p("a/b")))
        fails("not_found", fs.open(p("nope")))
        fails("not_found", fs.create(p("no/such/file")))
        fails("not_found", fs.append(p("no/such/file")))

        -- a failure about two paths names both
        local _, both = fs.rename(p("nope"), p("x"))
        check(both.path == p("nope") and both.to == p("x"), tostring(both))
        check(both.message == p("nope") .. " -> " .. p("x") .. ": " .. both.reason, both.message)
        -- and one about no path names none
        local _, none = fs.remove_dir_all("")
        check(none.kind == "invalid_input" and none.path == nil and none.message == none.reason, tostring(none))
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reading_writing_and_directories() {
    run(r#"
        check(ok(fs.write(p("f"), "hello")) == true)
        check(ok(fs.read(p("f"))) == "hello")
        ok(fs.write(p("f"), "bye"))
        check(ok(fs.read(p("f"))) == "bye", "write replaces")
        ok(fs.write(p("bin"), "\0\1\2\254\255"))
        check(ok(fs.read(p("bin"))) == "\0\1\2\254\255", "binary-safe")
        ok(fs.write(p("empty"), ""))
        check(ok(fs.read(p("empty"))) == "")
        check(ok(fs.create_dir(p("d"))) == true)
        fails("already_exists", fs.create_dir(p("d")))
        check(ok(fs.create_dir_all(p("d"))) == true)
        check(ok(fs.create_dir_all(p("a/b/c"))) == true)
        local value, err = fs.create_dir_all(p("f/x"))
        check(value == nil and err ~= nil, "a file is not a parent")
        fails(if windows then "permission_denied" else "is_directory", fs.read(p("d")))
        local _, empty = fs.read("")
        check(empty ~= nil, "an empty path is an error, not the current directory")
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_reads_and_create_and_append_write() {
    run(r#"
        ok(fs.write(p("f"), "one\ntwo\n"))
        local reader = ok(fs.open(p("f")))
        check(reader:read_line() == "one" and reader:read_all() == "two\n")

        local writer = ok(fs.create(p("w")))
        check(ok(fs.read(p("w"))) == "", "create makes the file at once")
        check(ok(writer:write("hello ")) == true and ok(writer:write("world")) == true)
        check(ok(writer:flush()) == true)
        check(ok(fs.read(p("w"))) == "hello world")
        local again = ok(fs.create(p("w")))
        ok(again:write("x"))
        ok(again:flush())
        check(ok(fs.read(p("w"))) == "x", "create empties an existing file")

        local more = ok(fs.append(p("w")))
        ok(more:write("yz"))
        ok(more:flush())
        check(ok(fs.read(p("w"))) == "xyz", "append keeps what is there")
        local fresh = ok(fs.append(p("new")))
        ok(fresh:write("n"))
        ok(fresh:flush())
        check(ok(fs.read(p("new"))) == "n", "append creates a missing file")

        -- a file is a reader and a writer like any other
        local copied = ok(fs.create(p("copy")))
        check(ok(io.copy(ok(fs.open(p("w"))), copied)) == 3)
        ok(copied:flush())
        check(ok(fs.read(p("copy"))) == "xyz")
        fails(if windows then "permission_denied" else "is_directory", fs.create(dir))
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
        check(m.kind == "file" and m.len == 5)
        check(m.modified ~= nil and m.modified > 1e9, "modified is a recent epoch time")
        check(type(m.readonly) == "boolean")
        check(m.created == nil or m.created > 1e9)
        check(windows == (m.mode == nil), "mode is the Unix bits")
        local d = ok(fs.metadata(dir))
        check(d.kind == "dir")
        check(ok(fs.symlink_metadata(p("f"))).kind == "file")

        ok(fs.set_readonly(p("f"), true))
        check(ok(fs.metadata(p("f"))).readonly == true)
        ok(fs.set_readonly(p("f"), false))
        check(ok(fs.metadata(p("f"))).readonly == false)
        if windows then fails("unsupported", fs.set_mode(p("f"), 420)) end
        fails("invalid_input", fs.set_mode(p("f"), 4096))
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
        check(entries[1].kind == "file" and entries[1].depth == 1)
        check(entries[3].kind == "dir")
        check(entries[3].path == p("sub"))
        check(names(ok(fs.read_dir(entries[3].path))) == "inner")
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
        check(all[1].depth == 1 and all[4].depth == 2 and all[6].depth == 3)
        check(all[6].path == path.join(path.join(path.join(p("t"), "sub"), "deeper"), "d"), all[6].path)
        check(#ok(fs.walk(p("t"), { max_depth = 1 })) == 3)
        check(#ok(fs.walk(p("t"), { max_depth = 0 })) == 0)
        check(#ok(fs.walk(p("t"), { max_depth = 4294967295 })) == 6)
        check(#ok(fs.walk(p("t"), {})) == 6 and #ok(fs.walk(p("t"), nil)) == 6)
        for _, bad in { -1, 0/0, 1.5, 1/0, "2", 2^32 } do
          refused("std:fs.walk: argument #2 'opts'", fs.walk, p("t"), { max_depth = bad })
        end
        refused("std:fs.walk: argument #2 'opts'", fs.walk, p("t"), { depth = 2 })
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
        ok(fs.write(p("none"), ""))
        check(ok(fs.copy(p("none"), p("none2"))) == 0)
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
        for _, op in { fs.rename, fs.move_path } do
          ok(fs.write(p("a"), "a"))
          check(ok(op(p("a"), p("b"))) == true)
          check(fs.exists(p("a")) == false and ok(fs.read(p("b"))) == "a")
          ok(fs.write(p("a"), "new"))
          ok(op(p("a"), p("b")))
          check(ok(fs.read(p("b"))) == "new", "an existing file is replaced")
          ok(fs.create_dir(p("full")))
          ok(fs.write(p("full/x"), "x"))
          ok(fs.write(p("a"), "a"))
          fails(if windows then "permission_denied" else "is_directory", op(p("a"), p("full")))
          check(ok(fs.read(p("a"))) == "a", "a refused move changes nothing")
          ok(fs.remove_file(p("a")))
          ok(fs.remove_file(p("b")))
          ok(fs.remove_dir_all(p("full")))
        end
        ok(fs.create_dir_all(p("tree/sub")))
        ok(fs.write(p("tree/sub/f"), "f"))
        ok(fs.move_path(p("tree"), p("moved")))
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
async fn options_refuse_typos_and_wrong_types_instead_of_ignoring_them() {
    run(r#"
        ok(fs.write(p("a"), "a"))
        refused("std:fs.remove_file: argument #2 'opts'", fs.remove_file, p("a"), { missing = true })
        refused("missing_ok", fs.remove_file, p("a"), { missing = true })
        refused("std:fs.remove_file: argument #2 'opts'", fs.remove_file, p("a"), { missing_ok = "yes" })
        refused("std:fs.remove_file: argument #2 'opts'", fs.remove_file, p("a"), { missing_ok = 1 })
        refused("std:fs.remove_file: argument #2 'opts'", fs.remove_file, p("a"), { true })
        refused("std:fs.remove_dir: argument #2 'opts'", fs.remove_dir, p("a"), { recursive = true })
        refused("std:fs.remove_dir_all: argument #2 'opts'", fs.remove_dir_all, p("a"), { recursive = true })
        refused("std:fs.watch: argument #2 'opts'", fs.watch, dir, { recursiv = true })
        refused("std:fs.watch: argument #2 'opts'", fs.watch, dir, { recursive = "yes" })
        for _, bad in { -1, 2^63, 1e300, 1.5, "420" } do
          refused("std:fs.set_mode: argument #2 'mode'", fs.set_mode, p("a"), bad)
        end
        refused("std:fs.set_readonly: argument #2 'readonly'", fs.set_readonly, p("a"), "no")
        check(fs.exists(p("a")) == true, "a refused call does nothing")
        check(ok(fs.remove_file(p("a"), {})) == true, "an empty table is the defaults")
        ok(fs.write(p("a"), "a"))
        check(ok(fs.remove_file(p("a"), nil)) == true, "nil is the defaults")
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_argument_type_is_a_lua_error_never_a_panic() {
    let lua = make_engine().unwrap();
    for call in [
        "fs.read({})",
        "fs.read()",
        "fs.write('a')",
        "fs.open({ path = 'a', mode = 'r' })",
        "fs.copy('a')",
        "fs.copy_dir('a', {})",
        "fs.walk('a', 5)",
        "fs.remove_dir_all('a', 'recursive')",
        "fs.read_dir(function() end)",
        "fs.metadata(nil)",
        "fs.set_mode('a')",
        "fs.set_readonly('a')",
        "fs.read('a', 'b')",
    ] {
        let outcome: mlua::Result<mlua::MultiValue> =
            lua.load(format!("local fs = require('std:fs'); return {call}")).eval_async().await;
        assert!(outcome.is_err(), "{call} should raise a Lua error");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hostile_paths_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let lua = make_engine().unwrap();
    lua.globals().set("dir", dir.path().to_str().unwrap()).unwrap();
    lua.load(
        r#"
        local fs = require("std:fs")
        local long = ("x"):rep(5000)
        for _, bad in { "", "a\0b", "\0", long, ("a/"):rep(3000) } do
          local label = tostring(#tostring(bad))
          local v, err = fs.read(bad)
          assert(v == nil and err.kind ~= nil, label .. ": " .. tostring(err))
          assert(fs.metadata(bad) == nil)
          assert(fs.exists(bad) == nil or fs.exists(bad) == false)
          assert(fs.read_dir(bad) == nil)
          assert(fs.walk(bad) == nil)
          assert(fs.remove_file(bad) == nil)
          assert(fs.remove_dir(bad) == nil)
          assert(fs.remove_dir_all(bad) == nil)
          assert(fs.copy(bad, bad) == nil)
          assert(fs.copy_dir(bad, bad) == nil)
          assert(fs.move_path(bad, bad) == nil)
          assert(fs.rename(bad, bad) == nil)
          assert(fs.symlink(bad, bad) == nil)
          assert(fs.read_link(bad) == nil)
          assert(fs.canonicalize(bad) == nil)
          assert(fs.set_readonly(bad, true) == nil)
          assert(fs.open(bad) == nil)
          assert(fs.watch(bad) == nil)
        end
        for _, call in { fs.read, fs.exists, fs.metadata, fs.create, fs.watch, fs.remove_dir_all } do
          local _, err = call("a\0b")
          assert(err.kind == "invalid_input" and err.reason:find("NUL", 1, true), tostring(err))
        end
        local _, err = fs.write("a\0b", "x")
        assert(err.kind == "invalid_input" and err.message:find("NUL", 1, true), tostring(err))
        "#,
    )
    .exec_async()
    .await
    .unwrap();
}

/// Where a path is text, a string that is not UTF-8 is not a path: it is refused as an argument, with its place.
#[cfg(not(unix))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_path_that_is_not_utf8_is_refused_as_an_argument_where_a_path_is_text() {
    run(r#"
        refused("std:fs.read: argument #1 'path': a path must be valid UTF-8", fs.read, "caf\233\255")
        refused("std:fs.copy: argument #2 'to': a path must be valid UTF-8", fs.copy, p("a"), "\255")
    "#)
    .await;
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
        check(ok(fs.symlink_metadata(p("copy/loop"))).kind == "symlink")
        check(ok(fs.remove_dir_all(p("copy"))) == true)
        check(ok(fs.remove_dir_all(p("t"))) == true)
        check(fs.exists(p("t")) == false)
        -- a link that loops on itself: following it fails, the link's own metadata does not
        check(select(1, fs.metadata(p("l1"))) == nil and select(2, fs.metadata(p("l1"))) ~= nil)
        check(ok(fs.symlink_metadata(p("l1"))).kind == "symlink")
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
        check(ok(fs.symlink_metadata(p("dirlink"))).kind == "symlink")
        check(ok(fs.metadata(p("dirlink"))).kind == "dir", "metadata follows")
        check(ok(fs.metadata(p("filelink"))).len == 1)
        check(ok(fs.symlink_metadata(p("dangling"))).kind == "symlink")
        fails("not_found", fs.metadata(p("dangling")))
        check(ok(fs.exists(p("dangling"))) == false, "a dangling link does not exist")
        check(path.file_name(ok(fs.canonicalize(p("dirlink")))) == "real")
        local listed = {}
        for _, e in ok(fs.read_dir(dir)) do listed[e.file_name] = e.kind end
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
        check(ok(fs.symlink_metadata(p("dst/loop"))).kind == "symlink")
        check(ok(fs.read_link(p("dst/loop"))) == dir)
        -- a trailing separator is dropped, so a link is renamed as a link
        ok(fs.symlink("real", p("lnk")))
        check(ok(fs.rename(p("lnk") .. "/", p("moved"))) == true)
        check(fs.exists(p("real/keep")) == true, "the target stays")
        check(ok(fs.symlink_metadata(p("moved"))).kind == "symlink")
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permissions_are_read_and_written_as_unix_bits() {
    run(r#"
        ok(fs.write(p("f"), "x"))
        local m = ok(fs.metadata(p("f")))
        check(type(m.mode) == "number" and m.mode > 0 and m.mode <= 4095, "mode is the permission bits")
        check(ok(fs.set_mode(p("f"), 493)) == true)
        check(ok(fs.metadata(p("f"))).mode == 493)
        ok(fs.set_readonly(p("f"), true))
        local r = ok(fs.metadata(p("f")))
        check(r.readonly == true and r.mode == 365, "readonly clears the write bits")
        ok(fs.set_readonly(p("f"), false))
        local w = ok(fs.metadata(p("f")))
        check(w.readonly == false and w.mode == 493, "and gives the owner write access back")
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_that_are_not_utf8_round_trip_through_read_dir() {
    use std::os::unix::ffi::OsStrExt;
    // some filesystems (APFS on macOS) refuse a name that is not UTF-8: nothing to round-trip there
    let probe = tempfile::tempdir().unwrap();
    if std::fs::write(probe.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9\xff")), "").is_err() {
        return;
    }
    run(r#"
        local odd = p("caf\233\255")
        ok(fs.write(odd, "odd"))
        local entries = ok(fs.read_dir(dir))
        check(#entries == 1 and entries[1].file_name == "caf\233\255")
        check(ok(fs.read(entries[1].path)) == "odd")
        check(#ok(fs.walk(dir)) == 1)
        check(ok(fs.copy(entries[1].path, p("copy\255"))) == 3)
        check(ok(fs.remove_file(entries[1].path)) == true)
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
    lua.globals().set("dir", dir.path().to_str().unwrap()).unwrap();
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
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap()).await.unwrap();
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

/// Luau refuses a yield inside these. A waiting call made there must fail and must not have done anything: the
/// job starts only after the call has suspended, so what is untouched here is untouched deterministically.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_long_calls_fail_where_luau_refuses_a_yield_and_do_nothing() {
    run(r#"
        ok(fs.write(p("src"), "NEW"))
        ok(fs.write(p("dst"), "OLD precious"))
        ok(fs.create_dir_all(p("tree/sub")))
        ok(fs.write(p("tree/sub/f"), "f"))
        ok(fs.write(p("m"), "m"))

        -- a callback of string.gsub
        check(pcall(string.gsub, "abc", "b", function() fs.copy(p("src"), p("dst")) return "x" end) == false)
        check(pcall(string.gsub, "abc", "b", function() fs.copy_dir(p("tree"), p("treecopy")) return "x" end) == false)
        -- a table.sort comparator
        check(pcall(table.sort, { 1, 2, 3 }, function(a, b) fs.move_path(p("m"), p("moved")) return a < b end) == false)
        check(pcall(table.sort, { 1, 2, 3 }, function(a, b) fs.walk(dir) return a < b end) == false)
        check(pcall(table.sort, { 1, 2, 3 }, function(a, b) fs.read_dir(dir) return a < b end) == false)
        check(pcall(string.gsub, "abc", "b", function() fs.watch(dir) return "x" end) == false)
        -- a metamethod
        local meta = setmetatable({}, { __index = function() fs.remove_dir_all(p("tree")) return 1 end })
        check(pcall(function() return meta.anything end) == false)

        check(ok(fs.read(p("dst"))) == "OLD precious", "copy changed the destination")
        check(fs.exists(p("treecopy")) == false, "copy_dir made its destination")
        check(fs.exists(p("moved")) == false and fs.exists(p("m")) == true, "move moved the file")
        check(fs.exists(p("tree/sub/f")) == true, "remove_dir_all removed the tree")

        -- the functions that run in place are fine in all of them
        check(pcall(table.sort, { 1, 2, 3 }, function(a, b) fs.metadata(dir) return a < b end) == true)
        -- and the same calls work where the call can wait
        check(ok(fs.copy(p("src"), p("dst"))) == 3)
        check(ok(fs.read(p("dst"))) == "NEW")
    "#)
    .await;
}

/// Helpers for the watch tests: `next_change(w, kind, path)` reads events until one of `kind` at `path` arrives
/// (skipping others: a platform may report one change as several), failing after 10 seconds; it never sleeps.
const WATCH_HELPERS: &str = r#"
    local ten = time.Duration.from_secs(10)
    local function next_change(w, kind, at)
      local started = time.Instant.now()
      while started:elapsed():as_secs_f64() < 10 do
        local ev, err = w:receive(ten)
        check(err == nil, tostring(err))
        check(ev ~= nil, "the watch ended")
        if ev.kind == kind and ev.path == at then return ev end
      end
      error("no " .. kind .. " event for " .. at)
    end
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_reports_create_modify_remove_and_rename_under_the_given_path() {
    run(&format!(
        r#"{WATCH_HELPERS}
        local w = ok(fs.watch(dir))
        ok(fs.write(p("a"), "1"))
        next_change(w, "create", p("a"))
        ok(fs.write(p("a"), "22"))
        next_change(w, "modify", p("a"))
        ok(fs.remove_file(p("a")))
        next_change(w, "remove", p("a"))
        ok(fs.write(p("b"), "x"))
        next_change(w, "create", p("b"))
        ok(fs.rename(p("b"), p("c")))
        next_change(w, "rename", p("c"))
        w:close()
    "#
    ))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_sees_subdirectories_only_when_recursive() {
    run(&format!(
        r#"{WATCH_HELPERS}
        ok(fs.create_dir(p("sub")))
        local inner = path.join(p("sub"), "x")
        local flat = ok(fs.watch(dir))
        local deep = ok(fs.watch(dir, {{ recursive = true }}))
        ok(fs.write(inner, "1"))
        next_change(deep, "create", inner)
        -- the flat watch reports the marker, and nothing inside sub before it
        ok(fs.write(p("marker"), "1"))
        while true do
          local ev = ok(flat:receive(ten))
          check(ev.path ~= inner, "a flat watch reported a change inside a subdirectory")
          if ev.path == p("marker") then break end
        end
        flat:close()
        deep:close()
    "#
    ))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_a_watch_ends_its_events() {
    run(r##"
        local w = ok(fs.watch(dir))
        w:close()
        ok(fs.write(p("after"), "1"))
        local ev, err = w:receive()
        check(ev == nil and err == nil, "a closed watch has no events")
        check(select("#", w:receive(time.Duration.zero())) == 2)
        w:close() -- again: nothing
    "##)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_refuses_what_it_cannot_watch() {
    run(r#"
        fails("not_found", fs.watch(p("nope")))
        local _, err = fs.watch(p("nope"))
        check(err.op == "std:fs.watch" and err.path == p("nope"), tostring(err))
        ok(fs.write(p("f"), "x"))
        fails("not_a_directory", fs.watch(p("f")))
        fails("invalid_input", fs.watch("a\0b"))

        local w = ok(fs.watch(dir))
        -- once what is already queued is taken (FSEvents may report a change from just before the watch started)
        local zero = time.Duration.zero()
        local ev, late = w:receive(zero)
        for _ = 1, 100 do
          if ev == nil then break end
          ev, late = w:receive(zero)
        end
        check(late.kind == "timed_out" and late.op == "std:fs.Watcher:receive" and late.path == dir, tostring(late))
        refused("std:fs.Watcher:receive: argument #1 'timeout': expected a Duration", w.receive, w, 5)
        refused("std:fs.Watcher:receive: argument #1 'timeout': expected a Duration", w.receive, w, "soon")
        w:close()
    "#)
    .await;
}
