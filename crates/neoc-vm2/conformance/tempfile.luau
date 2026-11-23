-- lib:tempfile on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the
-- harness. Everything it makes is under the system's temporary directory and is removed: closed here, or collected.
-- What an error is, the second names (`TempFile.new`, `TempDir.new`, `gettempdir`) and what `TempFile:close`
-- returns differ on purpose: see tempfile.vm2.luau.
local tempfile = require("lib:tempfile")
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")

local out = {}

local file = assert(tempfile.tempfile())
local file_path = file:path()
out.file = {
	type(file_path),
	fs.exists(file_path),
	assert(fs.metadata(file_path)) ~= nil,
	path.strip_prefix(file_path, env.temp_dir()) ~= nil,
	path.is_absolute(file_path),
	tostring(file) == "TempFile(" .. file_path .. ")",
	file:path() == file_path,
}
assert(fs.write(file_path, "content"))
out.file_content = fs.read(file_path)
file:close()
file:close()
out.file_closed = { fs.exists(file_path), file:path() == file_path }

local dir = assert(tempfile.tempdir())
local dir_path = dir:path()
out.dir = {
	fs.exists(dir_path),
	#assert(fs.read_dir(dir_path)),
	path.strip_prefix(dir_path, env.temp_dir()) ~= nil,
	tostring(dir) == "TempDir(" .. dir_path .. ")",
}
assert(fs.write(path.join(dir_path, "inner"), "x"))
out.dir_close = { dir:close(), dir:close(), fs.exists(dir_path) }

-- names
local named = assert(tempfile.tempfile({ prefix = "neoc-", suffix = ".tmp" }))
local name = assert(path.file_name(named:path()))
local named_dir = assert(tempfile.tempdir({ prefix = "neoc-", suffix = ".d" }))
local dir_name = assert(path.file_name(named_dir:path()))
out.names = { name:sub(1, 5), name:sub(-4), #name > 9, dir_name:sub(1, 5), dir_name:sub(-2), name ~= path.file_name(assert(tempfile.tempfile({ prefix = "neoc-", suffix = ".tmp" })):path()) }
named:close()

-- a parent of the script's choosing
local inside = assert(tempfile.tempfile({ dir = named_dir:path() }))
local inside_dir = assert(tempfile.tempdir({ dir = named_dir:path(), prefix = "sub" }))
out.parent = { path.parent(inside:path()) == named_dir:path(), path.parent(inside_dir:path()) == named_dir:path(), #assert(fs.read_dir(named_dir:path())) }
inside:close()
inside_dir:close()
out.parent_after = #assert(fs.read_dir(named_dir:path()))
named_dir:close()

-- keep: the file and the directory outlive their handles
local kept_file = assert(tempfile.tempfile())
local kept_dir = assert(tempfile.tempdir())
local kept_file_path, kept_dir_path = kept_file:keep(), kept_dir:keep()
kept_file:close()
kept_dir:close()
out.keep = {
	kept_file_path == kept_file:path(),
	kept_dir_path == kept_dir:path(),
	fs.exists(kept_file_path),
	fs.exists(kept_dir_path),
}
-- a failure is `(nil, err)`; what `err` is differs
local function failed(value: unknown, err: unknown): { boolean }
	return { value == nil, err ~= nil }
end
out.keep_again = { failed(kept_file:keep()), failed(kept_dir:keep()), failed(file:keep()), failed(dir:keep()) }
assert(fs.remove_file(kept_file_path))
assert(fs.remove_dir(kept_dir_path))

out.missing_parent = { failed(tempfile.tempfile({ dir = "/does/not/exist/anywhere" })), failed(tempfile.tempdir({ dir = "/does/not/exist/anywhere" })) }
out.empty_options = { tempfile.tempfile({}) ~= nil, tempfile.tempdir({}) ~= nil, tempfile.tempfile(nil) ~= nil }

return out
