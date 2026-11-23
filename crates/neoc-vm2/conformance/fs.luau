-- std:fs on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- It works in a directory of its own under the system's temporary directory and removes it at the end.
-- What an error is, `open`, `Metadata`, `DirEntry`, the permission functions, `move` and `watch` differ on purpose:
-- see fs.vm2.luau.
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")

local dir = path.join(env.temp_dir(), "neoc-conformance-fs-" .. random.uuid_v4())
local function p(name: string): string
	return path.join(dir, name)
end

-- a call's answer as plain values: the value (`false` for nil) and whether there was no error
local function answer(value: unknown, err: unknown): { unknown }
	return { if value == nil then false else value, err == nil }
end

local function body()
	local out = {}
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
	out.metadata = { fs.metadata(p("f")) ~= nil, fs.symlink_metadata(p("f")) ~= nil, answer(fs.metadata(p("nope"))) }

	out.create_dir = { answer(fs.create_dir(p("d"))), answer(fs.create_dir(p("d"))), answer(fs.create_dir(p("x/y"))) }
	out.create_dir_all = { answer(fs.create_dir_all(p("a/b/c"))), answer(fs.create_dir_all(p("a/b/c"))), answer(fs.exists(p("a/b/c"))) }

	fs.write(p("a/one"), "1")
	fs.write(p("a/b/two"), "22")
	fs.write(p("a/b/c/three"), "333")
	out.read_dir = { #assert(fs.read_dir(p("a"))), #assert(fs.read_dir(p("d"))), answer(fs.read_dir(p("nope"))), answer(fs.read_dir(p("f"))) }
	out.walk = {
		#assert(fs.walk(p("a"))),
		#assert(fs.walk(p("a"), { max_depth = 1 })),
		#assert(fs.walk(p("a"), { max_depth = 0 })),
		#assert(fs.walk(p("a"), {})),
		answer(fs.walk(p("nope"))),
		answer(fs.walk(p("f"))),
	}

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

	out.canonicalize = { fs.canonicalize(p("a/b/../b")) == fs.canonicalize(p("a/b")), answer(fs.canonicalize(p("nope"))) }
	out.read_link = answer(fs.read_link(p("bin")))
	out.nul = { answer(fs.read("a\0b")), answer(fs.write("a\0b", "x")), answer(fs.exists("a\0b")), answer(fs.read_dir("a\0b")) }
	out.empty_path = { answer(fs.read("")), answer(fs.metadata("")), answer(fs.read_dir("")) }
	out.wrong_arguments = { (pcall(fs.read, {})), (pcall(fs.read)), (pcall(fs.copy, "a")), (pcall(fs.walk, "a", 5)), (pcall(fs.metadata, nil)) }
	return out
end

local ok, out = pcall(body)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, out)
out.cleaned = answer(fs.exists(dir))
return out
