--!strict
-- lib:archive as a script sees it: its answers (archive.json), then what it asserts. How an archive is compressed is
-- read from its first bytes (`layer`) instead of from its length.
local archive = require("lib:archive")
local fs = require("std:fs")
local tempfile = require("lib:tempfile")
local out = {}

-- one way to say each thing for both formats: `Tar` and `Zip` have the same functions, but a zip made in memory
-- takes no options
local function api(Type: any, create_takes_options: boolean)
  return {
    create = function(entries: any, options: any?)
      if create_takes_options then
        return Type.create(entries, options)
      end
      return Type.create(entries)
    end,
    create_file = function(path: string, entries: any, options: any?)
      return Type.create_file(path, entries, options)
    end,
    list = function(bytes: string)
      return Type.from_bytes(bytes):list()
    end,
    list_file = function(path: string)
      local opened, err = Type.open(path)
      if opened == nil then
        return nil, err
      end
      return opened:list()
    end,
    extract = function(bytes: string, dest: string, options: any?)
      return Type.from_bytes(bytes):extract(dest, options)
    end,
    extract_file = function(path: string, dest: string, options: any?)
      return assert(Type.open(path)):extract(dest, options)
    end,
    read = function(bytes: string, name: string, options: any?)
      return Type.from_bytes(bytes):read(name, options)
    end,
  }
end
local tar, zip = api(archive.Tar, true), api(archive.Zip, false)

local function fails(value, err)
  return { value == nil, err ~= nil }
end

-- the entries as plain rows, sorted by name (no timestamp: see above)
local function rows(entries)
  local list = {}
  for _, entry in entries do
    local ratio = entry.compression_ratio
    table.insert(list, {
      entry.name,
      entry.size,
      entry.is_dir,
      if ratio == nil then "none" else ratio,
    })
  end
  table.sort(list, function(a, b)
    return a[1] < b[1]
  end)
  return list
end

-- which compression wraps the bytes, by their magic number
local function layer(bytes: string): string
  local a, b, c, d = string.byte(bytes, 1, 4)
  if a == 0x1f and b == 0x8b then
    return "gzip"
  elseif a == 0x28 and b == 0xb5 and c == 0x2f and d == 0xfd then
    return "zstd"
  end
  return "none"
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
    layer(bytes),
    rows(assert(tar.list(bytes))),
    tar.read(bytes, "app/main.lua"),
    tar.read(bytes, "blob.bin") == "\0\1\2\254\255\127\128",
    fails(tar.read(bytes, "nope")),
  } :: { any }
end

-- a directory is every file under it; strip_prefix shortens names
out.tar_directory = rows(assert(tar.list((assert(tar.create { { path = root .. "/sub", archive_path = "pkg" } })))))
out.tar_strip = rows(
  assert(
    tar.list(
      (assert(tar.create({ { path = root .. "/main.lua", archive_path = "src/main.lua" } }, { strip_prefix = "src/" })))
    )
  )
)
out.tar_no_entries = rows(assert(tar.list((assert(tar.create {})))))

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
    layer((assert(fs.read(path)))),
    rows(assert(tar.list_file(path))),
    tar.extract_file(path, to),
    fs.read(to .. "/main.lua"),
    fs.read(to .. "/Cargo.toml"),
  }
end
out.tar_extension_clash = fails(
  tar.create_file(root .. "/x.tar.gz", { { path = root .. "/main.lua", archive_path = "m" } }, { compression = "zstd" })
)
out.tar_extension_agrees = tar.create_file(
  root .. "/y.tar.gz",
  { { path = root .. "/main.lua", archive_path = "m" } },
  { compression = "gzip" }
)
out.tar_explicit = tar.create_file(
  root .. "/z.tar",
  { { path = root .. "/main.lua", archive_path = "m" } },
  { compression = "gzip" }
)

-- include
local both = assert(tar.create {
  { path = root .. "/main.lua", archive_path = "main.lua" },
  { path = root .. "/Cargo.toml", archive_path = "Cargo.toml" },
  { path = root .. "/sub/deep.txt", archive_path = "sub/deep.txt" },
})
local lua_only = dest()
out.include = {
  tar.extract(both, lua_only, { include = { "*.lua" } }),
  fs.exists(lua_only .. "/main.lua"),
  fs.exists(lua_only .. "/Cargo.toml"),
}
local nested = dest()
out.include_crosses_separators =
  { tar.extract(both, nested, { include = { "*.txt", "*.toml" } }), fs.exists(nested .. "/sub/deep.txt") }
local everything = dest()
out.include_empty_is_everything = tar.extract(both, everything, { include = {} })
out.include_invalid = fails(tar.extract(both, dest(), { include = { "[" } }))

-- what leaves the destination
out.tar_writer_refuses_traversal = fails(tar.create { { path = root .. "/main.lua", archive_path = "../escape.lua" } })
local evil = assert(zip.create { ["../../evil.txt"] = "pwned" })
out.zip_escape = fails(zip.extract(evil, dest()))
local safe_dest = dest()
out.zip_normalises = {
  zip.extract((assert(zip.create { ["safe/../safe/ok.txt"] = "fine" })), safe_dest),
  fs.read(safe_dest .. "/safe/ok.txt"),
}
out.zip_absolute = fails(zip.extract((assert(zip.create { ["/abs.txt"] = "x" })), dest()))
out.zip_drive = fails(zip.extract((assert(zip.create { ["C:\\abs.txt"] = "x" })), dest()))

-- the cap on what an archive expands to
local big_zip = assert(zip.create { ["big.txt"] = string.rep("a", 200000) })
local big_tar =
  assert(tar.create({ { path = root .. "/big.txt", archive_path = "big.txt" } }, { compression = "gzip" }))
out.cap = {
  fails(zip.extract(big_zip, dest(), { max_output_size = 1024 })),
  fails(zip.read(big_zip, "big.txt", { max_output_size = 1024 })),
  fails(tar.extract(big_tar, dest(), { max_output_size = 1024 })),
  fails(tar.read(big_tar, "big.txt", { max_output_size = 1024 })),
  #assert(zip.read(big_zip, "big.txt", { max_output_size = 300000 })),
  #assert(tar.read(big_tar, "big.txt", { max_output_size = 300000 })),
  fails(zip.read(big_zip, "big.txt", { max_output_size = 0 })),
} :: { any }

-- zip
local zipped = assert(zip.create { ["a.txt"] = string.rep("x", 1000), ["b.txt"] = "small", ["dir/c.bin"] = "\0\255" })
out.zip = {
  rows(assert(zip.list(zipped))),
  zip.read(zipped, "a.txt") == string.rep("x", 1000),
  zip.read(zipped, "dir/c.bin") == "\0\255",
  fails(zip.read(zipped, "nope.txt")),
} :: { any }
local unzipped = dest()
out.zip_extract =
  { zip.extract(zipped, unzipped), fs.read(unzipped .. "/b.txt"), fs.read(unzipped .. "/dir/c.bin") == "\0\255" }
local zip_path = root .. "/out.zip"
local zip_written = zip.create_file(zip_path, { root .. "/main.lua", root .. "/sub" }, { strip_prefix = root })
local from_file = dest()
out.zip_file = {
  zip_written,
  rows(assert(zip.list_file(zip_path))),
  zip.extract_file(zip_path, from_file),
  fs.read(from_file .. "/sub/deep.txt"),
}

-- what is not an archive, or not there
out.not_archives = {
  fails(zip.list("not a zip")),
  fails(tar.list("not a tar, and long enough to look like a header that is not one")),
  fails(tar.list("\31\139 not gzip")),
  fails(tar.create { { path = root .. "/nope", archive_path = "nope" } }),
  fails(zip.create_file(root .. "/no/such/dir/out.zip", {})),
}

scratch:close()

-- what it asserts
do
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
  local zip =
    archive.Zip.from_bytes((assert(archive.Zip.create { ["b.txt"] = "small", ["a.txt"] = string.rep("x", 1000) })))
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

  local evil = archive.Zip.from_bytes((assert(archive.Zip.create { ["../../evil.txt"] = "pwned" })))
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

  local _, clash = archive.Tar.create_file(
    root .. "/x.tar.gz",
    { { path = root .. "/main.lua" } },
    { compression = "zstd" }
  )
  assert(clash ~= nil and clash.kind == "compression_mismatch")
  if clash.kind == "compression_mismatch" then
    assert(clash.compression == "zstd" and clash.extension == "gzip")
  end

  local _, traversal = archive.Tar.create { { path = root .. "/main.lua", archive_path = "../escape.lua" } }
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
end

return out
