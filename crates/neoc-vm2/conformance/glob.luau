-- lib:glob on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- It scans a directory of its own under the system's temporary directory and removes it at the end.
-- What an error is, a refused option and the shape of a scan's result differ on purpose: see glob.vm2.luau.
local glob = require("lib:glob")
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")

local out = {}

out.is_match = {
	glob.is_match("src/**/*.rs", "src/lua/fs.rs"),
	glob.is_match("src/**/*.rs", "README.md"),
	glob.is_match("*.rs", "a.rs"),
	glob.is_match("*.{rs,toml}", "Cargo.toml"),
	glob.is_match("[ab]?.txt", "a1.txt"),
	glob.is_match("[ab]?.txt", "c1.txt"),
	glob.is_match("**/*.rs", "src\\lua\\fs.rs"),
	glob.is_match("", ""),
	glob.is_match("naïve/*", "naïve/値"),
}
out.dotfiles = {
	glob.is_match("*", ".gitignore"),
	glob.is_match(".git*", ".gitignore"),
	glob.is_match("**/*.rs", "src/.cache/foo.rs"),
	glob.is_match("**/.cache/*.rs", "src/.cache/foo.rs"),
}
-- a failure is `(nil, err)`; what `err` is differs
local function failed(value: unknown, err: unknown): { boolean }
	return { value == nil, err ~= nil }
end
local deep = string.rep("{a,", 130) .. "a" .. string.rep("}", 130)
out.invalid = {
	failed(glob.is_match("src/[*.rs", "src/foo.rs")),
	failed(glob.Glob.new("src/[*.rs")),
	failed(glob.GlobSet.new({ "valid", "inv[alid" })),
	failed(glob.is_match(deep, "a")),
	failed(glob.Glob.new(string.rep("a", 4097))),
	failed(glob.scan(deep, { cwd = "." })),
}
out.at_the_bounds = { glob.Glob.new(string.rep("a", 4096)) ~= nil, glob.Glob.new(string.rep("{a,b}", 256)) ~= nil }

local compiled = assert(glob.Glob.new("src/**/*.rs"))
out.glob = { compiled:is_match("src/lua/fs.rs"), compiled:is_match("lib/x.rs"), tostring(compiled) }

local set = assert(glob.GlobSet.new({ "*.rs", "*.toml", "Cargo.*", ".git*" }))
out.set = {
	set:is_match("Cargo.toml"),
	set:is_match("x.md"),
	set:is_match(".gitignore"),
	set:matches("Cargo.toml"),
	set:matches("main.rs"),
	set:matches("x.md"),
	set:matches(".gitignore"),
	set:len(),
}
local empty = assert(glob.GlobSet.new({}))
out.empty_set = { empty:is_match("anything"), empty:matches("anything"), empty:len() }

-- A scan's matches: neoc-vm2 returns `{ paths, errors }`, neoc-vm the paths (and the errors second).
local function paths(found: any): { string }
	return found.paths or found
end

local dir = path.join(env.temp_dir(), "neoc-conformance-glob-" .. random.uuid_v4())
local function scans()
	local function p(name: string): string
		return path.join(dir, name)
	end
	assert(fs.create_dir_all(p("src/deep")))
	assert(fs.create_dir_all(p("target")))
	assert(fs.create_dir_all(p(".cache")))
	for _, name in { "b.txt", "a.txt", ".hidden.txt", "README.md", "src/main.rs", "src/lib.rs", "src/deep/x.rs", "target/t.rs", ".cache/c.rs" } do
		assert(fs.write(p(name), ""))
	end
	local compiled_txt = assert(glob.Glob.new("*.txt"))
	local absolute = paths(assert(glob.scan("src/*.rs", { cwd = dir, absolute = true, limit = 1 })))
	return {
		paths(assert(glob.scan("*.txt", { cwd = dir }))),
		paths(assert(glob.scan("*", { cwd = dir }))),
		paths(assert(glob.scan(".*", { cwd = dir }))),
		paths(assert(glob.scan("**/*.rs", { cwd = dir }))),
		paths(assert(glob.scan("**/*.rs", { cwd = dir, ignore = { "**/target/**", "src/deep/**" } }))),
		paths(assert(glob.scan("**/*.rs", { cwd = dir, limit = 2 }))),
		paths(assert(glob.scan("**/*.rs", { cwd = dir, limit = 0 }))),
		paths(assert(glob.scan("*", { cwd = dir, type = "dir" }))),
		paths(assert(glob.scan("*", { cwd = dir, type = "any" }))),
		paths(assert(glob.scan("**", { cwd = dir, type = "dir" }))),
		paths(assert(glob.scan("*.none", { cwd = dir }))),
		paths(assert(glob.scan("*.txt", { cwd = dir, follow_symlinks = true }))),
		paths(assert(compiled_txt:scan({ cwd = dir }))),
		{ #absolute, path.is_absolute(absolute[1]), absolute[1]:sub(-12) },
		failed(glob.scan("*", { cwd = p("nope") })),
		failed(glob.scan("*", { cwd = p("a.txt") })),
		failed(compiled_txt:scan({ cwd = p("nope") })),
		failed(glob.scan("*", { cwd = dir, ignore = { "inv[alid" } })),
	}
end
local ok, scanned = pcall(scans)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, scanned)
out.scan = scanned

out.wrong_arguments = { (pcall(glob.is_match, "*")), (pcall(glob.is_match, {}, "a")), (pcall(glob.Glob.new)), (pcall(glob.GlobSet.new, "a")) }

return out
