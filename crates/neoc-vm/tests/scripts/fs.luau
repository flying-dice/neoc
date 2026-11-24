--!strict
-- std:fs as a script sees it: its answers (fs.json), then what it asserts. It works in a directory of its own under the
-- system's temporary directory and removes it at the end.
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")

local dir = path.join(env.temp_dir(), "neoc-scripts-fs-" .. random.uuid_v4())
local function p(name: string): string
  return path.join(dir, name)
end

-- a call's answer as plain values: the value (`false` for nil) and whether there was no error
local function answer(value: unknown, err: unknown): { unknown }
  return { if value == nil then false else value, err == nil }
end

local function body(): { [string]: any }
  local out: { [string]: any } = {}
  out.setup = answer(fs.create_dir(dir))

  out.write = { answer(fs.write(p("f"), "hello")), answer(fs.read(p("f"))) }
  fs.write(p("f"), "bye")
  out.replaced = answer(fs.read(p("f")))
  fs.write(p("bin"), "\0\1\2\254\255")
  out.binary = fs.read(p("bin")) == "\0\1\2\254\255"
  fs.write(p("empty"), "")
  out.empty = answer(fs.read(p("empty")))
  out.read_missing = answer(fs.read(p("nope")))
  out.write_no_parent = answer(fs.write(p("no/such/f"), "x"))

  out.exists = { answer(fs.exists(dir)), answer(fs.exists(p("f"))), answer(fs.exists(p("nope"))) }
  out.metadata =
    { fs.metadata(p("f")) ~= nil, fs.symlink_metadata(p("f")) ~= nil, answer(fs.metadata(p("nope"))) } :: { any }

  out.create_dir = { answer(fs.create_dir(p("d"))), answer(fs.create_dir(p("d"))), answer(fs.create_dir(p("x/y"))) }
  out.create_dir_all =
    { answer(fs.create_dir_all(p("a/b/c"))), answer(fs.create_dir_all(p("a/b/c"))), answer(fs.exists(p("a/b/c"))) }

  fs.write(p("a/one"), "1")
  fs.write(p("a/b/two"), "22")
  fs.write(p("a/b/c/three"), "333")
  out.read_dir = {
    #assert(fs.read_dir(p("a"))),
    #assert(fs.read_dir(p("d"))),
    answer(fs.read_dir(p("nope"))),
    answer(fs.read_dir(p("f"))),
  } :: { any }
  out.walk = {
    #assert(fs.walk(p("a"))),
    #assert(fs.walk(p("a"), { max_depth = 1 })),
    #assert(fs.walk(p("a"), { max_depth = 0 })),
    #assert(fs.walk(p("a"), {})),
    answer(fs.walk(p("nope"))),
    answer(fs.walk(p("f"))),
  } :: { any }

  out.copy = {
    answer(fs.copy(p("f"), p("g"))),
    answer(fs.read(p("g"))),
    answer(fs.copy(p("a/b/two"), p("g"))),
    answer(fs.read(p("g"))),
    answer(fs.copy(p("f"), p("f"))),
    answer(fs.read(p("f"))),
    answer(fs.copy(p("f"), p("d"))),
    answer(fs.copy(p("d"), p("e"))),
    answer(fs.copy(p("nope"), p("e"))),
    answer(fs.copy(p("f"), p("no/such/e"))),
  }
  out.copy_dir = {
    answer(fs.copy_dir(p("a"), p("copy"))),
    answer(fs.read(p("copy/b/c/three"))),
    answer(fs.copy_dir(p("a"), p("copy"))),
    answer(fs.copy_dir(p("a"), p("a/b/inside"))),
    answer(fs.copy_dir(p("f"), p("other"))),
    answer(fs.exists(p("a/b/inside"))),
  }

  out.rename = {
    answer(fs.rename(p("g"), p("h"))),
    answer(fs.exists(p("g"))),
    answer(fs.read(p("h"))),
    answer(fs.rename(p("h"), p("f"))),
    answer(fs.read(p("f"))),
    answer(fs.rename(p("copy"), p("renamed"))),
    answer(fs.read(p("renamed/one"))),
    answer(fs.rename(p("nope"), p("x"))),
  }

  out.remove_file = {
    answer(fs.remove_file(p("f"))),
    answer(fs.remove_file(p("f"))),
    answer(fs.remove_file(p("f"), { missing_ok = true })),
    answer(fs.remove_file(p("f"), {})),
    answer(fs.remove_file(p("d"))),
  }
  out.remove_dir = {
    answer(fs.remove_dir(p("a"))),
    answer(fs.remove_dir(p("d"))),
    answer(fs.remove_dir(p("d"))),
    answer(fs.remove_dir(p("d"), { missing_ok = true })),
    answer(fs.remove_dir(p("bin"))),
  }
  out.remove_dir_all = {
    answer(fs.remove_dir_all(p("renamed"))),
    answer(fs.exists(p("renamed"))),
    answer(fs.remove_dir_all(p("renamed"))),
    answer(fs.remove_dir_all(p("renamed"), { missing_ok = true })),
    answer(fs.remove_dir_all(p("bin"))),
    answer(fs.remove_dir_all("")),
    answer(fs.remove_dir_all(".")),
    answer(fs.remove_dir_all("..")),
    answer(fs.remove_dir_all("/")),
  }

  out.canonicalize =
    { fs.canonicalize(p("a/b/../b")) == fs.canonicalize(p("a/b")), answer(fs.canonicalize(p("nope"))) } :: { any }
  out.read_link = answer(fs.read_link(p("bin")))
  out.nul =
    { answer(fs.read("a\0b")), answer(fs.write("a\0b", "x")), answer(fs.exists("a\0b")), answer(fs.read_dir("a\0b")) }
  out.empty_path = { answer(fs.read("")), answer(fs.metadata("")), answer(fs.read_dir("")) }
  out.wrong_arguments = {
    (pcall(fs.read :: any, {})),
    (pcall(fs.read :: any)),
    (pcall(fs.copy :: any, "a")),
    (pcall(fs.walk :: any, "a", 5)),
    (pcall(fs.metadata :: any, nil)),
  }
  return out
end

local ok, out = pcall(body)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, out)
out.cleaned = answer(fs.exists(dir))

-- what it asserts
do
  local io = require("std:io")
  local time = require("std:time")
  local csv = require("lib:csv")

  local windows = env.family == "windows"
  local dir = path.join(env.temp_dir(), "neoc-scripts-fs-asserts-" .. random.uuid_v4())
  local function p(name: string): string
    return path.join(dir, name)
  end

  local function body(): ...unknown
    assert(fs.create_dir(dir))

    -- a typed error: the kind, the path, the operating system's words
    local value, err = fs.read(p("nope"))
    assert(value == nil and err ~= nil)
    assert(err.kind == "not_found" and err.op == "std:fs.read" and err.path == p("nope") and err.to == nil)
    assert(err.message == p("nope") .. ": " .. err.reason)
    assert(tostring(err) == "std:fs.read: not_found: " .. err.message)
    local _, both = fs.rename(p("nope"), p("x"))
    assert(both ~= nil and both.kind == "not_found" and both.path == p("nope") and both.to == p("x"))
    local _, nul = fs.read("a\0b")
    assert(
      nul ~= nil
        and nul.kind == "invalid_input"
        and nul.path == nil
        and nul.reason == "a path cannot contain a NUL byte"
    )
    local _, exists = fs.create_dir(dir)
    assert(exists ~= nil and exists.kind == "already_exists")
    assert((fs :: any).error_kind == nil and (fs :: any).error_message == nil)

    -- open reads; create and append write
    assert(fs.write(p("f"), "one\ntwo\n"))
    local reader = assert(fs.open(p("f")))
    assert(reader:read_line() == "one" and reader:read_all() == "two\n")
    local writer = assert(fs.create(p("w")))
    assert(writer:write("x") == true and writer:flush() == true)
    local more = assert(fs.append(p("w")))
    assert(more:write("yz") and more:flush())
    assert(fs.read(p("w")) == "xyz")
    local copied = assert(fs.create(p("copy")))
    assert(io.copy((assert(fs.open(p("w")))), copied) == 3 and copied:flush())
    local _, no_file = fs.open(p("nope"))
    assert(no_file ~= nil and no_file.kind == "not_found" and no_file.op == "std:fs.open")
    assert(not pcall(function()
      fs.open({ path = p("f"), mode = "r" } :: any) -- deliberate refusal test: the table open took
    end))

    -- a file is a reader for the modules that take one
    assert(fs.write(p("rows.csv"), "name,age\nada,36\ngrace,45\n"))
    local types: { [string]: lib_csv_ColumnType } = { age = "number" }
    local people = assert(csv.reader((assert(fs.open(p("rows.csv")))), { types = types }))
    local total = 0
    for person in people:records():items() do
      local age = person.age
      assert(type(age) == "number")
      total += age
    end
    assert(total == 81)

    -- Metadata and DirEntry are records
    local meta = assert(fs.metadata(p("f")))
    assert(meta.kind == "file" and meta.len == 8 and meta.readonly == false)
    assert(type(meta.modified) == "number" and (meta.mode == nil) == windows)
    assert(assert(fs.metadata(dir)).kind == "dir")
    assert(fs.create_dir(p("sub")))
    assert(fs.write(p("sub/inner"), "i"))
    local entries = assert(fs.read_dir(dir))
    local names = {}
    for i, entry in entries do
      names[i] = entry.file_name .. ":" .. entry.kind .. ":" .. entry.depth
    end
    assert(
      table.concat(names, ",") == "copy:file:1,f:file:1,rows.csv:file:1,sub:dir:1,w:file:1",
      table.concat(names, ",")
    )
    assert(entries[4].path == p("sub"))
    local walked = assert(fs.walk(dir))
    assert(#walked == 6 and walked[5].file_name == "inner" and walked[5].depth == 2)
    assert(walked[5].path == path.join(p("sub"), "inner"))

    -- the two things set_permissions did
    assert(fs.set_readonly(p("f"), true) == true)
    assert(assert(fs.metadata(p("f"))).readonly == true)
    assert(fs.set_readonly(p("f"), false))
    local mode_ok, mode_err = fs.set_mode(p("f"), 420)
    if windows then
      assert(mode_ok == nil and mode_err ~= nil and mode_err.kind == "unsupported")
    else
      assert(mode_ok == true and assert(fs.metadata(p("f"))).mode == 420)
    end
    local _, too_big = fs.set_mode(p("f"), 4096)
    assert(too_big ~= nil and too_big.kind == "invalid_input")
    assert((fs :: any).set_permissions == nil)

    -- move: a rename that survives a filesystem boundary
    assert(fs.move(p("w"), p("moved")) == true)
    assert(fs.exists(p("w")) == false and fs.read(p("moved")) == "xyz")
    assert(fs.move(p("sub"), p("sub2")) and fs.read(p("sub2/inner")) == "i")
    local _, missing = fs.move(p("nope"), p("x"))
    assert(missing ~= nil and missing.kind == "not_found" and missing.op == "std:fs.move")

    -- a wrong option is a wrong argument
    -- deliberate refusal tests: the casts are options that do not exist or have the wrong type
    local wrong: { () -> ...unknown } = {
      function()
        return fs.remove_file(p("f"), { missing = true } :: any)
      end,
      function()
        return fs.remove_file(p("f"), { missing_ok = "yes" } :: any)
      end,
      function()
        return fs.walk(dir, { depth = 2 } :: any)
      end,
      function()
        return fs.walk(dir, { max_depth = -1 })
      end,
      function()
        return fs.walk(dir, { max_depth = 1.5 })
      end,
      function()
        return fs.watch(dir, { recursive = "yes" } :: any)
      end,
    }
    for _, call in wrong do
      local ok, refused = pcall(call)
      assert(not ok and tostring(refused):find("argument #2 'opts'", 1, true), tostring(refused))
    end
    assert(fs.exists(p("f")) == true)

    -- a watcher's timeout is a Duration
    local watcher = assert(fs.watch(dir))
    local event, late = watcher:receive(time.Duration.zero())
    for _ = 1, 100 do
      if event == nil then
        break
      end
      event, late = watcher:receive(time.Duration.zero())
    end
    assert(late ~= nil and late.kind == "timed_out" and late.op == "std:fs.Watcher:receive" and late.path == dir)
    assert(fs.write(p("seen"), "1"))
    local seen = false
    for _ = 1, 100 do
      local change = assert(watcher:receive(time.Duration.from_secs(10)))
      if change.path == p("seen") then
        seen = change.kind == "create" or change.kind == "modify"
        break
      end
    end
    assert(seen)
    assert(not pcall(watcher.receive, watcher, 5 :: any))
    watcher:close()
    local closed, closed_err = watcher:receive()
    assert(closed == nil and closed_err == nil)
  end

  local ok, failure = pcall(body)
  fs.remove_dir_all(dir, { missing_ok = true })
  assert(ok, failure)
end

return out
