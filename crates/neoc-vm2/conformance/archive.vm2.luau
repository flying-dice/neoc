--!strict
-- intentional: an archive is an object: Tar.open(path) / Tar.from_bytes(bytes) with :list(), :extract(dest, options?) and :read(name, options?) replace tar_list, tar_list_bytes, tar_extract and tar_extract_bytes, whose first argument was a path or the archive's bytes by whether a file of that name existed (Zip is the same); Tar.create / Tar.create_file and Zip.create / Zip.create_file are tar_create_bytes / tar_create and zip_create_bytes / zip_create; Zip.create writes its entries in the order of their names; an option or an entry of the wrong shape is refused as an argument; every failure is the typed ArchiveError (io, not_a_file_or_directory, invalid_name, write_failed, compression_mismatch, invalid_pattern, max_output_size_zero, corrupt, unsafe_entry, unsafe_link, too_large, not_found, unsupported).
local archive = require("lib:archive")
local fs = require("std:fs")
local tempfile = require("lib:tempfile")

local scratch = assert(tempfile.tempdir())
local root = (scratch:path():gsub("\\", "/"))
assert(fs.write(root .. "/main.lua", "print('hi')"))
assert(fs.write(root .. "/Cargo.toml", "[package]"))

-- a tar in memory: made, listed, read
local bytes = assert(archive.Tar.create({
	{ path = root .. "/main.lua", archive_path = "app/main.lua" },
	{ path = root .. "/Cargo.toml", archive_path = "app/Cargo.toml" },
}, { compression = "zstd" }))
local tar = archive.Tar.from_bytes(bytes)
local entries = assert(tar:list())
assert(#entries == 2 and entries[1].name == "app/main.lua" and entries[1].size == 11)
assert(entries[1].is_dir == false and entries[1].compression_ratio == nil and entries[1].timestamp > 0)
assert(tar:read("app/main.lua") == "print('hi')")

-- a tar on disk: written, opened, extracted
local path = root .. "/bundle.tar.gz"
assert(archive.Tar.create_file(path, { { path = root .. "/main.lua", archive_path = "main.lua" } }) == true)
local opened = assert(archive.Tar.open(path))
assert(assert(opened:extract(root .. "/out", { include = { "*.lua" } })) == 1)
assert(fs.read(root .. "/out/main.lua") == "print('hi')")

-- a bytes argument is bytes, and a path a path: neither is guessed
local as_bytes = archive.Tar.from_bytes(path)
local _, not_a_tar = as_bytes:list()
assert(not_a_tar ~= nil and not_a_tar.kind == "corrupt", "a path given as bytes is not read from disk")
local _, no_file = archive.Tar.open(root .. "/nope.tar")
assert(no_file ~= nil and no_file.kind == "io" and no_file.op == "lib:archive.Tar.open")
if no_file.kind == "io" then
	assert(no_file.path == root .. "/nope.tar" and #no_file.reason > 0)
end

-- a zip: its entries are in the order of their names, each with its ratio
local zip = archive.Zip.from_bytes((assert(archive.Zip.create({ ["b.txt"] = "small", ["a.txt"] = string.rep("x", 1000) }))))
local listed = assert(zip:list())
assert(listed[1].name == "a.txt" and listed[2].name == "b.txt")
local ratio = assert(listed[1].compression_ratio)
assert(ratio < 1 and listed[1].size == 1000)
assert(zip:read("b.txt") == "small")
assert(archive.Zip.create_file(root .. "/out.zip", { root .. "/main.lua" }, { strip_prefix = root }) == true)
assert(assert(assert(archive.Zip.open(root .. "/out.zip")):list())[1].name == "main.lua")

-- typed errors
local _, missing = zip:read("nope.txt")
assert(missing ~= nil and missing.kind == "not_found")
if missing.kind == "not_found" then
	assert(missing.name == "nope.txt")
end
assert(tostring(missing) == 'lib:archive.Zip:read: not_found: entry "nope.txt" not found in archive')

local evil = archive.Zip.from_bytes((assert(archive.Zip.create({ ["../../evil.txt"] = "pwned" }))))
local _, unsafe = evil:extract(root .. "/out")
assert(unsafe ~= nil and unsafe.kind == "unsafe_entry")
if unsafe.kind == "unsafe_entry" then
	assert(unsafe.name == "../../evil.txt" and unsafe.reason == "path escapes the destination directory")
end

local _, large = zip:read("a.txt", { max_output_size = 10 })
assert(large ~= nil and large.kind == "too_large")
if large.kind == "too_large" then
	assert(large.max_output_size == 10)
end
local _, zero = zip:extract(root .. "/out", { max_output_size = 0 })
assert(zero ~= nil and zero.kind == "max_output_size_zero")

local _, clash = archive.Tar.create_file(root .. "/x.tar.gz", { { path = root .. "/main.lua" } }, { compression = "zstd" })
assert(clash ~= nil and clash.kind == "compression_mismatch")
if clash.kind == "compression_mismatch" then
	assert(clash.compression == "zstd" and clash.extension == "gzip")
end

local _, traversal = archive.Tar.create({ { path = root .. "/main.lua", archive_path = "../escape.lua" } })
assert(traversal ~= nil and traversal.kind == "invalid_name")
local _, pattern = tar:extract(root .. "/out", { include = { "[" } })
assert(pattern ~= nil and pattern.kind == "invalid_pattern")
local _, corrupt = archive.Zip.from_bytes("not a zip"):list()
assert(corrupt ~= nil and corrupt.kind == "corrupt")

-- what is not an option or an entry is refused as an argument, with its place
local function refused(place: string, f: () -> ...unknown)
	local ok, err = pcall(f)
	assert(not ok and tostring(err):find(place, 1, true), tostring(err))
end
refused("lib:archive.Tar.create: argument #2 'options'", function()
	return archive.Tar.create({}, { compression = "lz4" } :: any)
end)
refused("lib:archive.Tar.create: argument #1 'entries'", function()
	return archive.Tar.create({ "main.lua" } :: any)
end)
refused("lib:archive.Zip.create: argument #1 'entries'", function()
	return archive.Zip.create({ a = 5 } :: any)
end)
refused("lib:archive.Tar:extract: argument #2 'options'", function()
	return tar:extract(root, { max_output_size = -1 })
end)
refused("lib:archive.Zip:extract: argument #2 'options'", function()
	return zip:extract(root, { include = "*.lua" } :: any)
end)
-- the functions that took a path or bytes are gone
local module = archive :: any
assert(module.tar_extract == nil and module.tar_list_bytes == nil and module.zip_create_bytes == nil)

scratch:close()
