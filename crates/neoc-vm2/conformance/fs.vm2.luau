-- intentional: an error is a typed FsError (kind, path, to, reason; error_kind and error_message are gone), open(path) reads while create(path) and append(path) write (it was open { path, mode }), Metadata and DirEntry are plain records (meta.len, entry.file_name) instead of objects with a method per field, set_permissions is set_mode and set_readonly, Watcher:receive takes a std:time Duration, and a wrong option is refused as a wrong argument with its place (it was a returned invalid_input).
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local io = require("std:io")
local time = require("std:time")
local csv = require("lib:csv")
local random = require("lib:random")

local windows = env.family == "windows"
local dir = path.join(env.temp_dir(), "neoc-conformance-fs-vm2-" .. random.uuid_v4())
local function p(name: string): string
	return path.join(dir, name)
end

local function body()
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
	assert(nul ~= nil and nul.kind == "invalid_input" and nul.path == nil and nul.reason == "a path cannot contain a NUL byte")
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
	assert(not pcall(fs.open, { path = p("f"), mode = "r" }))

	-- a file is a reader for the modules that take one
	assert(fs.write(p("rows.csv"), "name,age\nada,36\ngrace,45\n"))
	local people = assert(csv.reader((assert(fs.open(p("rows.csv")))), { types = { age = "number" } }))
	local total = 0
	for person in people:records():items() do
		total += person.age :: number
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
	assert(table.concat(names, ",") == "copy:file:1,f:file:1,rows.csv:file:1,sub:dir:1,w:file:1", table.concat(names, ","))
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
	for _, call in
		{
			function() return fs.remove_file(p("f"), { missing = true } :: any) end,
			function() return fs.remove_file(p("f"), { missing_ok = "yes" } :: any) end,
			function() return fs.walk(dir, { depth = 2 } :: any) end,
			function() return fs.walk(dir, { max_depth = -1 }) end,
			function() return fs.walk(dir, { max_depth = 1.5 }) end,
			function() return fs.watch(dir, { recursive = "yes" } :: any) end,
		}
	do
		local ok, refused = pcall(call)
		assert(not ok and tostring(refused):find("argument #2 'opts'", 1, true), tostring(refused))
	end
	assert(fs.exists(p("f")) == true)

	-- a watcher's timeout is a Duration
	local watcher = assert(fs.watch(dir))
	local event, late = watcher:receive(time.Duration.zero())
	for _ = 1, 100 do
		if event == nil then break end
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
