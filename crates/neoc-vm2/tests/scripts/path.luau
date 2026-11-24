--!strict
-- std:path as a script sees it: its answers, then what it asserts. Paths are the platform's, so the answers are a file
-- per platform (path.unix.json, path.windows.json).
local path = require("std:path")
local out = {}

-- nil answers are kept apart from strings
local function maybe(value: string?): string | boolean
  return if value == nil then false else value
end

out.separator = path.separator
out.join = {
  path.join("a", "b"),
  path.join(path.join("a", "b"), "c.txt"),
  path.join("a", ".."),
  path.join("a", "/b"),
  path.join("a", ""),
  path.join("", "a"),
  path.join("a\0b", "c"),
  path.join("a"),
  path.join("a", "b", "c.txt"),
  path.join("a", "b", "..", "c", ""),
  path.join("a", "/b", "c"),
  path.join("", "", "a"),
}

local samples = {
  "",
  ".",
  "..",
  "/",
  "a",
  "a/b.txt",
  "a/./b//c",
  "a/b/..",
  "a/../..",
  "../a",
  "./a",
  "/a/../..",
  "/..",
  "/a/b/c",
  "a/b.tar.gz",
  ".bashrc",
  "a/.bashrc",
  "a/b.",
  "a/..",
  "C:\\dir\\file.txt",
  "C:relative",
  "\\\\server\\share\\x",
  "dir with space/naïve.txt",
}
for _, sample in samples do
  out[sample] = {
    normalize = path.normalize(sample),
    parent = maybe(path.parent(sample)),
    file_name = maybe(path.file_name(sample)),
    file_stem = maybe(path.file_stem(sample)),
    extension = maybe(path.extension(sample)),
    is_absolute = path.is_absolute(sample),
    components = path.components(sample),
    with_md = path.with_extension(sample, "md"),
    without_extension = path.with_extension(sample, ""),
    renamed = path.with_file_name(sample, "c.md"),
  }
end

out.strip_prefix = {
  maybe(path.strip_prefix("/a/b/c", "/a/b")),
  maybe(path.strip_prefix("/a/bc", "/a/b")),
  maybe(path.strip_prefix("/a", "/a")),
  maybe(path.strip_prefix("/a", "/b")),
  maybe(path.strip_prefix("a/b", "a")),
}

out.absolute = { path.absolute("x/./y"), path.absolute(path.join("x", "y")) }
local nothing, why = path.absolute("")
out.absolute_empty = { nothing == nil, why ~= nil }

local long = string.rep("a/", 50000)
out.long = { #path.normalize(long), #path.components(long) }

out.wrong_arguments = {
  (pcall(path.normalize :: any, {})),
  (pcall(path.parent :: any, nil)),
  (pcall(path.join :: any)),
  (pcall(path.join :: any, "a", {})),
}

-- what it asserts
do
  -- join is a base and any number of parts; each part is a path
  local sep = path.separator
  assert(path.join("a") == "a")
  assert(path.join("a", "b") == "a" .. sep .. "b")
  assert(path.join("a", "b", "c.txt") == "a" .. sep .. "b" .. sep .. "c.txt")
  local no_base = pcall(function()
    (path.join :: any)()
  end)
  assert(not no_base)
  local ok, not_a_part = pcall(function()
    path.join("a", "b", nil :: any)
  end)
  assert(not ok and tostring(not_a_part):find("std:path.join: argument #3 'parts': expected a string", 1, true))

  -- typed errors
  local value, separator = path.with_extension("a/b.txt", "x/y")
  assert(value == nil and separator ~= nil)
  assert(separator.kind == "separator" and separator.op == "std:path.with_extension")
  assert(tostring(separator) == "std:path.with_extension: separator: an extension cannot contain a path separator")
  local absolute, empty = path.absolute("")
  assert(absolute == nil and empty ~= nil)
  assert(empty.kind == "empty_path" and empty.op == "std:path.absolute")

  -- a wrong argument is refused with its place
  local refused_ok, refused = pcall(function()
    path.normalize({} :: any)
  end)
  assert(
    not refused_ok and tostring(refused):find("std:path.normalize: argument #1 'path': expected a string", 1, true)
  )
end

return out
