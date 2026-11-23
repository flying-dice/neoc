-- std:path on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- `join` with one or several parts, and what an error is, differ on purpose: see path.vm2.luau.
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

out.wrong_arguments = { (pcall(path.normalize, {})), (pcall(path.parent, nil)), (pcall(path.join)) }

return out
