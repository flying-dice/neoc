--!strict
-- lib:glob as a script sees it: its answers (glob.json), then what it asserts. It scans a directory of its own under
-- the system's temporary directory and removes it at the end.
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
-- a failure is `(nil, err)`
local function failed(value: unknown, err: unknown): { boolean }
  return { value == nil, err ~= nil }
end
local deep = string.rep("{a,", 130) .. "a" .. string.rep("}", 130)
out.invalid = {
  failed(glob.is_match("src/[*.rs", "src/foo.rs")),
  failed(glob.Glob.new("src/[*.rs")),
  failed(glob.GlobSet.new { "valid", "inv[alid" }),
  failed(glob.is_match(deep, "a")),
  failed(glob.Glob.new(string.rep("a", 4097))),
  failed(glob.scan(deep, { cwd = "." })),
}
out.at_the_bounds = { glob.Glob.new(string.rep("a", 4096)) ~= nil, glob.Glob.new(string.rep("{a,b}", 256)) ~= nil }

local compiled = assert(glob.Glob.new("src/**/*.rs"))
out.glob = { compiled:is_match("src/lua/fs.rs"), compiled:is_match("lib/x.rs"), tostring(compiled) } :: { any }

local set = assert(glob.GlobSet.new { "*.rs", "*.toml", "Cargo.*", ".git*" })
out.set = {
  set:is_match("Cargo.toml"),
  set:is_match("x.md"),
  set:is_match(".gitignore"),
  set:matches("Cargo.toml"),
  set:matches("main.rs"),
  set:matches("x.md"),
  set:matches(".gitignore"),
  set:len(),
} :: { any }
local empty = assert(glob.GlobSet.new {})
out.empty_set = { empty:is_match("anything"), empty:matches("anything"), empty:len() } :: { any }

local dir = path.join(env.temp_dir(), "neoc-scripts-glob-" .. random.uuid_v4())
local function scans()
  local function p(name: string): string
    return path.join(dir, name)
  end
  assert(fs.create_dir_all(p("src/deep")))
  assert(fs.create_dir_all(p("target")))
  assert(fs.create_dir_all(p(".cache")))
  for _, name in
    {
      "b.txt",
      "a.txt",
      ".hidden.txt",
      "README.md",
      "src/main.rs",
      "src/lib.rs",
      "src/deep/x.rs",
      "target/t.rs",
      ".cache/c.rs",
    }
  do
    assert(fs.write(p(name), ""))
  end
  local compiled_txt = assert(glob.Glob.new("*.txt"))
  local absolute = assert(glob.scan("src/*.rs", { cwd = dir, absolute = true, limit = 1 })).paths
  return {
    assert(glob.scan("*.txt", { cwd = dir })).paths,
    assert(glob.scan("*", { cwd = dir })).paths,
    assert(glob.scan(".*", { cwd = dir })).paths,
    assert(glob.scan("**/*.rs", { cwd = dir })).paths,
    assert(glob.scan("**/*.rs", { cwd = dir, ignore = { "**/target/**", "src/deep/**" } })).paths,
    assert(glob.scan("**/*.rs", { cwd = dir, limit = 2 })).paths,
    assert(glob.scan("**/*.rs", { cwd = dir, limit = 0 })).paths,
    assert(glob.scan("*", { cwd = dir, type = "dir" })).paths,
    assert(glob.scan("*", { cwd = dir, type = "any" })).paths,
    assert(glob.scan("**", { cwd = dir, type = "dir" })).paths,
    assert(glob.scan("*.none", { cwd = dir })).paths,
    assert(glob.scan("*.txt", { cwd = dir, follow_symlinks = true })).paths,
    assert(compiled_txt:scan { cwd = dir }).paths,
    { #absolute, path.is_absolute(absolute[1]), absolute[1]:sub(-12) } :: { any },
    failed(glob.scan("*", { cwd = p("nope") })),
    failed(glob.scan("*", { cwd = p("a.txt") })),
    failed(compiled_txt:scan { cwd = p("nope") }),
    failed(glob.scan("*", { cwd = dir, ignore = { "inv[alid" } })),
  } :: { any }
end
local ok, scanned = pcall(scans)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, scanned)
out.scan = scanned

out.wrong_arguments = {
  (pcall(glob.is_match :: any, "*")),
  (pcall(glob.is_match :: any, {}, "a")),
  (pcall(glob.Glob.new :: any)),
  (pcall(glob.GlobSet.new :: any, "a")),
}

-- what it asserts
do
  -- a pattern that is not one
  local function invalid(op: string, value: unknown, err: lib_glob_ScanError?)
    assert(value == nil and err ~= nil)
    assert(err.kind == "invalid_pattern" and err.pattern == "src/[*.rs" and err.op == op)
    assert(tostring(err) == op .. ": invalid_pattern: src/[*.rs: " .. err.reason)
  end
  invalid("lib:glob.is_match", glob.is_match("src/[*.rs", "a"))
  invalid("lib:glob.Glob.new", glob.Glob.new("src/[*.rs"))
  invalid("lib:glob.GlobSet.new", glob.GlobSet.new { "ok", "src/[*.rs" })
  invalid("lib:glob.scan", glob.scan("src/[*.rs"))
  invalid("lib:glob.scan", glob.scan("*", { ignore = { "src/[*.rs" } }))

  -- the bounds: the pattern is cut in the error
  local _, long = glob.Glob.new(string.rep("a", 4097))
  assert(
    long ~= nil and long.kind == "invalid_pattern" and #long.pattern == 128 and long.reason:find("too long", 1, true)
  )
  local _, deep = glob.is_match(string.rep("{a,", 33) .. "a" .. string.rep("}", 33), "a")
  assert(deep ~= nil and deep.reason:find("nests", 1, true))
  local _, many = glob.is_match(string.rep("{a,b}", 257), "a")
  assert(many ~= nil and many.reason:find("more than 256", 1, true))

  -- a scan: one value, and typed failures
  local dir = path.join(env.temp_dir(), "neoc-scripts-glob-asserts-" .. random.uuid_v4())
  local function scans(): ...unknown
    assert(fs.create_dir(dir))
    assert(fs.write(path.join(dir, "b.txt"), ""))
    assert(fs.write(path.join(dir, "a.txt"), ""))
    local found, err = glob.scan("*.txt", { cwd = dir })
    assert(found ~= nil and err == nil)
    assert(table.concat(found.paths, ",") == "a.txt,b.txt" and #found.errors == 0)
    local again = assert(assert(glob.Glob.new("*.txt")):scan { cwd = dir, limit = 1 })
    assert(#again.paths == 1 and again.paths[1] == "a.txt" and #again.errors == 0)

    local missing = path.join(dir, "nope")
    local none, not_found = glob.scan("*", { cwd = missing })
    assert(none == nil and not_found ~= nil and not_found.kind == "not_found" and not_found.path == missing)
    assert(tostring(not_found) == "lib:glob.scan: not_found: directory not found: " .. missing)
    local file = path.join(dir, "a.txt")
    local _, not_dir = assert(glob.Glob.new("*")):scan { cwd = file }
    assert(
      not_dir ~= nil
        and not_dir.kind == "not_a_directory"
        and not_dir.path == file
        and not_dir.op == "lib:glob.Glob:scan"
    )
  end
  local ok, failure = pcall(scans)
  fs.remove_dir_all(dir, { missing_ok = true })
  assert(ok, failure)

  -- a wrong option is a wrong argument
  for _, opts in
    { { type = "block" }, { limit = -1 }, { limit = 1.5 }, { cwd = 5 }, { ignore = "x" }, { absolut = true } } :: { any }
  do -- deliberate refusal test: none is the options
    local refused_ok, refused = pcall(function()
      glob.scan("*", opts)
    end)
    assert(not refused_ok and tostring(refused):find("lib:glob.scan: argument #2 'opts'", 1, true), tostring(refused))
  end
end

return out
