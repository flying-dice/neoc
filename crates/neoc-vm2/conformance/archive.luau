-- lib:archive on neoc-vm and neoc-vm2: what both must answer the same. The functions were reshaped into archive
-- objects (archive.vm2.luau), so `api` below is the only place that knows which VM it is on; what an archive holds,
-- what is extracted and what is refused are compared. An entry's timestamp is the time it (or its file on disk) was
-- made, which differs between the two VMs' runs across a second boundary, so it is left out for tar and zip alike.
local archive = require("lib:archive") :: any
local fs = require("std:fs")
local tempfile = require("lib:tempfile")
local out = {}

local objects = archive.Tar ~= nil -- neoc-vm2
local api = {}
for _, format in { "tar", "zip" } do
	local Type = if objects then archive[if format == "tar" then "Tar" else "Zip"] else nil
	local function opened(path)
		return Type.open(path)
	end
	api[format] = {
		create = function(entries, options)
			if objects and format == "zip" then
				return Type.create(entries) -- a zip in memory has no options
			elseif objects then
				return Type.create(entries, options)
			end
			return archive[format .. "_create_bytes"](entries, options)
		end,
		create_file = function(path, entries, options)
			if objects then
				return Type.create_file(path, entries, options)
			end
			return archive[format .. "_create"](path, entries, options)
		end,
		list = function(bytes)
			if objects then
				return Type.from_bytes(bytes):list()
			end
			return archive[format .. "_list_bytes"](bytes)
		end,
		list_file = function(path)
			if objects then
				local a, err = opened(path)
				if a == nil then
					return nil, err
				end
				return a:list()
			end
			return archive[format .. "_list"](path)
		end,
		extract = function(bytes, dest, options)
			if objects then
				return Type.from_bytes(bytes):extract(dest, options)
			end
			return archive[format .. "_extract"](bytes, dest, options)
		end,
		extract_file = function(path, dest, options)
			if objects then
				return assert(opened(path)):extract(dest, options)
			end
			return archive[format .. "_extract"](path, dest, options)
		end,
		read = function(bytes, name, options)
			if objects then
				return Type.from_bytes(bytes):read(name, options)
			end
			return archive[format .. "_extract_bytes"](bytes, name, options)
		end,
	}
end
local tar, zip = api.tar, api.zip

local function fails(value, err)
	return { value == nil, err ~= nil }
end

-- the entries as plain rows, sorted by name
local function rows(entries, with_timestamp: boolean)
	local list = {}
	for _, entry in entries do
		local ratio = entry.compression_ratio
		table.insert(list, {
			entry.name,
			entry.size,
			entry.is_dir,
			if with_timestamp then entry.timestamp else 0,
			if ratio == nil then "none" else ratio,
		})
	end
	table.sort(list, function(a, b)
		return a[1] < b[1]
	end)
	return list
end

local scratch = assert(tempfile.tempdir())
local root = (scratch:path():gsub("\\", "/"))
assert(fs.write(root .. "/main.lua", "print('hi')"))
assert(fs.write(root .. "/Cargo.toml", "[package]"))
assert(fs.write(root .. "/blob.bin", "\0\1\2\254\255\127\128"))
assert(fs.create_dir(root .. "/sub"))
assert(fs.write(root .. "/sub/deep.txt", "deep"))
assert(fs.write(root .. "/big.txt", string.rep("a", 200000)))
local dest_count = 0
local function dest()
	dest_count += 1
	return root .. "/dest" .. dest_count
end

-- tar: each compression, read back without being told which
for _, compression in { "none", "gzip", "zstd" } do
	local bytes = assert(tar.create({
		{ path = root .. "/main.lua", archive_path = "app/main.lua" },
		{ path = root .. "/blob.bin", archive_path = "blob.bin" },
	}, { compression = compression }))
	out["tar_" .. compression] = {
		#bytes,
		rows(assert(tar.list(bytes)), false),
		tar.read(bytes, "app/main.lua"),
		tar.read(bytes, "blob.bin") == "\0\1\2\254\255\127\128",
		fails(tar.read(bytes, "nope")),
	}
end

-- a directory is every file under it; strip_prefix shortens names
out.tar_directory = rows(assert(tar.list((assert(tar.create({ { path = root .. "/sub", archive_path = "pkg" } }))))), false)
out.tar_strip = rows(
	assert(tar.list((assert(tar.create({ { path = root .. "/main.lua", archive_path = "src/main.lua" } }, { strip_prefix = "src/" }))))),
	true
)
out.tar_no_entries = rows(assert(tar.list((assert(tar.create({}))))), false)

-- tar to a file: compressed as its extension says
for _, name in { "bundle.tar", "bundle.tar.gz", "bundle.tgz", "bundle.tar.zst" } do
	local path = root .. "/" .. name
	local written = tar.create_file(path, {
		{ path = root .. "/main.lua", archive_path = "main.lua" },
		{ path = root .. "/Cargo.toml", archive_path = "Cargo.toml" },
	})
	local to = dest()
	out["file_" .. name] = {
		written,
		#assert(fs.read(path)),
		rows(assert(tar.list_file(path)), false),
		tar.extract_file(path, to),
		fs.read(to .. "/main.lua"),
		fs.read(to .. "/Cargo.toml"),
	}
end
out.tar_extension_clash = fails(tar.create_file(root .. "/x.tar.gz", { { path = root .. "/main.lua", archive_path = "m" } }, { compression = "zstd" }))
out.tar_extension_agrees = tar.create_file(root .. "/y.tar.gz", { { path = root .. "/main.lua", archive_path = "m" } }, { compression = "gzip" })
out.tar_explicit = tar.create_file(root .. "/z.tar", { { path = root .. "/main.lua", archive_path = "m" } }, { compression = "gzip" })

-- include
local both = assert(tar.create({
	{ path = root .. "/main.lua", archive_path = "main.lua" },
	{ path = root .. "/Cargo.toml", archive_path = "Cargo.toml" },
	{ path = root .. "/sub/deep.txt", archive_path = "sub/deep.txt" },
}))
local lua_only = dest()
out.include = { tar.extract(both, lua_only, { include = { "*.lua" } }), fs.exists(lua_only .. "/main.lua"), fs.exists(lua_only .. "/Cargo.toml") }
local nested = dest()
out.include_crosses_separators = { tar.extract(both, nested, { include = { "*.txt", "*.toml" } }), fs.exists(nested .. "/sub/deep.txt") }
local everything = dest()
out.include_empty_is_everything = tar.extract(both, everything, { include = {} })
out.include_invalid = fails(tar.extract(both, dest(), { include = { "[" } }))

-- what leaves the destination
out.tar_writer_refuses_traversal = fails(tar.create({ { path = root .. "/main.lua", archive_path = "../escape.lua" } }))
local evil = assert(zip.create({ ["../../evil.txt"] = "pwned" }))
out.zip_escape = fails(zip.extract(evil, dest()))
local safe_dest = dest()
out.zip_normalises = { zip.extract((assert(zip.create({ ["safe/../safe/ok.txt"] = "fine" }))), safe_dest), fs.read(safe_dest .. "/safe/ok.txt") }
out.zip_absolute = fails(zip.extract((assert(zip.create({ ["/abs.txt"] = "x" }))), dest()))
out.zip_drive = fails(zip.extract((assert(zip.create({ ["C:\\abs.txt"] = "x" }))), dest()))

-- the cap on what an archive expands to
local big_zip = assert(zip.create({ ["big.txt"] = string.rep("a", 200000) }))
local big_tar = assert(tar.create({ { path = root .. "/big.txt", archive_path = "big.txt" } }, { compression = "gzip" }))
out.cap = {
	fails(zip.extract(big_zip, dest(), { max_output_size = 1024 })),
	fails(zip.read(big_zip, "big.txt", { max_output_size = 1024 })),
	fails(tar.extract(big_tar, dest(), { max_output_size = 1024 })),
	fails(tar.read(big_tar, "big.txt", { max_output_size = 1024 })),
	#assert(zip.read(big_zip, "big.txt", { max_output_size = 300000 })),
	#assert(tar.read(big_tar, "big.txt", { max_output_size = 300000 })),
	fails(zip.read(big_zip, "big.txt", { max_output_size = 0 })),
}

-- zip
local zipped = assert(zip.create({ ["a.txt"] = string.rep("x", 1000), ["b.txt"] = "small", ["dir/c.bin"] = "\0\255" }))
out.zip = {
	rows(assert(zip.list(zipped)), false),
	zip.read(zipped, "a.txt") == string.rep("x", 1000),
	zip.read(zipped, "dir/c.bin") == "\0\255",
	fails(zip.read(zipped, "nope.txt")),
}
local unzipped = dest()
out.zip_extract = { zip.extract(zipped, unzipped), fs.read(unzipped .. "/b.txt"), fs.read(unzipped .. "/dir/c.bin") == "\0\255" }
local zip_path = root .. "/out.zip"
local zip_written = zip.create_file(zip_path, { root .. "/main.lua", root .. "/sub" }, { strip_prefix = root })
local from_file = dest()
out.zip_file = {
	zip_written,
	rows(assert(zip.list_file(zip_path)), false),
	zip.extract_file(zip_path, from_file),
	fs.read(from_file .. "/sub/deep.txt"),
}

-- what is not an archive, or not there
out.not_archives = {
	fails(zip.list("not a zip")),
	fails(tar.list("not a tar, and long enough to look like a header that is not one")),
	fails(tar.list("\31\139 not gzip")),
	fails(tar.create({ { path = root .. "/nope", archive_path = "nope" } })),
	fails(zip.create_file(root .. "/no/such/dir/out.zip", {})),
}

scratch:close()
return out
