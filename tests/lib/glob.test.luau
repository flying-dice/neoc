local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local glob = require("lib:glob")
local fs   = require("std:fs")
local env  = require("std:env")

-- Build a fresh, uniquely-named tempdir for each scan-style test so they
-- don't bleed into each other. std:fs has no remove API, so leakage to
-- /tmp is accepted — matches other tests in this suite (csv, compile).
local _counter = 0
local function fresh_tempdir(label)
    _counter += 1
    local path = string.format(
        "%s/neoc-glob-%d-%d-%s",
        env.temp_dir(),
        os.time(),
        _counter,
        label
    )
    local ok, err = fs.create_dir_all(path)
    assert(ok, err)
    return path
end

describe("glob", function()

    -- ── Flat: is_match ───────────────────────────────────────────────────────

    describe("is_match", function()
        test("basic match", function()
            local ok = glob.is_match("src/**/*.rs", "src/lua/fs.rs")
            expect(ok).toBeTruthy()
        end)

        test("no match", function()
            local ok = glob.is_match("src/**/*.rs", "README.md")
            expect(ok).toBeFalsy()
        end)

        test("character class", function()
            expect(glob.is_match("file.[ch]", "file.c")).toBeTruthy()
            expect(glob.is_match("file.[ch]", "file.h")).toBeTruthy()
            expect(glob.is_match("file.[ch]", "file.x")).toBeFalsy()
        end)

        test("alternation", function()
            expect(glob.is_match("*.{rs,toml}", "Cargo.toml")).toBeTruthy()
            expect(glob.is_match("*.{rs,toml}", "main.rs")).toBeTruthy()
            expect(glob.is_match("*.{rs,toml}", "README.md")).toBeFalsy()
        end)

        test("question mark", function()
            expect(glob.is_match("f?o", "foo")).toBeTruthy()
            expect(glob.is_match("f?o", "fo")).toBeFalsy()
        end)

        test("dotfile excluded by default", function()
            expect(glob.is_match("*", ".gitignore")).toBeFalsy()
        end)

        test("dotfile allowed when pattern is dotted", function()
            expect(glob.is_match(".git*", ".gitignore")).toBeTruthy()
        end)

        test("nested dotfile component filters whole path", function()
            -- `.cache` taints the path even though the leaf is plain.
            expect(glob.is_match("**/*.rs", "src/.cache/foo.rs")).toBeFalsy()
        end)

        test("nested dotfile allowed when pattern opts in", function()
            expect(glob.is_match("**/.cache/*.rs", "src/.cache/foo.rs")).toBeTruthy()
        end)

        test("backslash path normalised", function()
            expect(glob.is_match("src/**/*.rs", "src\\lib\\json.rs")).toBeTruthy()
        end)

        test("invalid pattern returns (nil, err)", function()
            local ok, err = glob.is_match("src/[*.rs", "src/foo.rs")
            expect(ok).toBeNil()
            expect(err).toContain("glob.is_match:")
        end)
    end)

    -- ── Glob UserData ────────────────────────────────────────────────────────

    describe("Glob", function()
        test("new + is_match", function()
            local g = glob.Glob.new("*.rs")
            expect(g:is_match("foo.rs")).toBeTruthy()
            expect(g:is_match("foo.md")).toBeFalsy()
        end)

        test("tostring returns original pattern", function()
            local g = glob.Glob.new("src/**/*.rs")
            expect(tostring(g)).toBe("src/**/*.rs")
        end)

        test("invalid pattern returns (nil, err)", function()
            local g, err = glob.Glob.new("src/[*.rs")
            expect(g).toBeNil()
            expect(err).toContain("glob.Glob.new:")
        end)

        test("reusable across calls", function()
            local g = glob.Glob.new("*.rs")
            for _ = 1, 3 do
                expect(g:is_match("a.rs")).toBeTruthy()
                expect(g:is_match("a.md")).toBeFalsy()
            end
        end)

        test("dotfile rule mirrors flat surface", function()
            expect(glob.Glob.new("*"):is_match(".gitignore")).toBeFalsy()
            expect(glob.Glob.new(".git*"):is_match(".gitignore")).toBeTruthy()
        end)
    end)

    -- ── GlobSet UserData ─────────────────────────────────────────────────────

    describe("GlobSet", function()
        test("matches returns 1-based indices", function()
            local s = glob.GlobSet.new({ "*.rs", "*.toml" })
            local idx = s:matches("Cargo.toml")
            expect(#idx).toBe(1)
            expect(idx[1]).toBe(2)
        end)

        test("matches returns all matching patterns", function()
            local s = glob.GlobSet.new({ "*.rs", "*", "src/*.rs" })
            local idx = s:matches("foo.rs")
            -- "*.rs" and "*" match; "src/*.rs" does not (no src/ prefix)
            expect(#idx).toBe(2)
            expect(idx[1]).toBe(1)
            expect(idx[2]).toBe(2)
        end)

        test("is_match short-circuits", function()
            local s = glob.GlobSet.new({ "*.rs", "*.toml" })
            expect(s:is_match("Cargo.toml")).toBeTruthy()
            expect(s:is_match("readme.txt")).toBeFalsy()
        end)

        test("len reports pattern count", function()
            expect(glob.GlobSet.new({ "a", "b", "c" }):len()).toBe(3)
        end)

        test("empty set matches nothing — not an error", function()
            local s, err = glob.GlobSet.new({})
            expect(err).toBeNil()
            expect(s:len()).toBe(0)
            expect(s:is_match("anything")).toBeFalsy()
            expect(#s:matches("anything")).toBe(0)
        end)

        test("rejects whole set when any pattern is invalid", function()
            local s, err = glob.GlobSet.new({ "valid", "inv[alid" })
            expect(s).toBeNil()
            expect(err).toContain("glob.GlobSet.new:")
        end)

        test("dotfile filter applied per-pattern", function()
            local s = glob.GlobSet.new({ "*", ".git*" })
            -- "*" wouldn't match .gitignore (dotfile rule); ".git*" would.
            local idx = s:matches(".gitignore")
            expect(#idx).toBe(1)
            expect(idx[1]).toBe(2)
        end)
    end)

    -- ── Flat: scan ───────────────────────────────────────────────────────────

    describe("scan", function()
        test("returns sorted results", function()
            local dir = fresh_tempdir("sorted")
            fs.write(dir .. "/b.txt", "")
            fs.write(dir .. "/a.txt", "")
            fs.write(dir .. "/c.txt", "")
            local r = glob.scan("*.txt", { cwd = dir })
            expect(#r).toBe(3)
            expect(r[1]).toBe("a.txt")
            expect(r[2]).toBe("b.txt")
            expect(r[3]).toBe("c.txt")
        end)

        test("recurses with ** pattern", function()
            local dir = fresh_tempdir("recurse")
            fs.create_dir_all(dir .. "/nested/deep")
            fs.write(dir .. "/top.rs", "")
            fs.write(dir .. "/nested/mid.rs", "")
            fs.write(dir .. "/nested/deep/leaf.rs", "")
            fs.write(dir .. "/nested/deep/skip.md", "")
            local r = glob.scan("**/*.rs", { cwd = dir })
            expect(#r).toBe(3)
            expect(r[1]).toBe("nested/deep/leaf.rs")
            expect(r[2]).toBe("nested/mid.rs")
            expect(r[3]).toBe("top.rs")
        end)

        test("empty directory returns empty array, not error", function()
            local dir = fresh_tempdir("empty")
            local r, err = glob.scan("*", { cwd = dir })
            expect(err).toBeNil()
            expect(#r).toBe(0)
        end)

        test("missing cwd returns (nil, err)", function()
            local r, err = glob.scan("*.rs", { cwd = "/no/such/place_xyzzy_neoc" })
            expect(r).toBeNil()
            expect(err).toContain("glob.scan: directory not found:")
        end)

        test("dotfiles excluded by default", function()
            local dir = fresh_tempdir("dotfile")
            fs.write(dir .. "/visible.txt", "")
            fs.write(dir .. "/.hidden", "")
            local r = glob.scan("*", { cwd = dir })
            expect(#r).toBe(1)
            expect(r[1]).toBe("visible.txt")
        end)

        test("dotfiles included when pattern is dotted", function()
            local dir = fresh_tempdir("dotted")
            fs.write(dir .. "/.gitignore", "")
            fs.write(dir .. "/regular", "")
            local r = glob.scan(".git*", { cwd = dir })
            expect(#r).toBe(1)
            expect(r[1]).toBe(".gitignore")
        end)

        test("limit truncates results", function()
            local dir = fresh_tempdir("limit")
            fs.write(dir .. "/a", "")
            fs.write(dir .. "/b", "")
            fs.write(dir .. "/c", "")
            local r = glob.scan("*", { cwd = dir, limit = 2 })
            expect(#r).toBe(2)
        end)

        test("limit = 0 returns empty array", function()
            local dir = fresh_tempdir("limit-zero")
            fs.write(dir .. "/a", "")
            local r = glob.scan("*", { cwd = dir, limit = 0 })
            expect(#r).toBe(0)
        end)

        test("limit < 0 returns (nil, err)", function()
            local r, err = glob.scan("*", { limit = -1 })
            expect(r).toBeNil()
            expect(err).toContain("limit must be >= 0")
        end)

        test("ignore excludes patterns", function()
            local dir = fresh_tempdir("ignore")
            fs.create_dir_all(dir .. "/keep")
            fs.create_dir_all(dir .. "/target/debug")
            fs.write(dir .. "/keep/a.rs", "")
            fs.write(dir .. "/target/debug/b.rs", "")
            local r = glob.scan("**/*.rs", {
                cwd    = dir,
                ignore = { "**/target/**" },
            })
            expect(#r).toBe(1)
            expect(r[1]).toBe("keep/a.rs")
        end)

        test("type = dir lists directories", function()
            local dir = fresh_tempdir("type-dir")
            fs.create_dir_all(dir .. "/sub")
            fs.write(dir .. "/file", "")
            local r = glob.scan("*", { cwd = dir, type = "dir" })
            expect(#r).toBe(1)
            expect(r[1]).toBe("sub")
        end)

        test("type = any lists both", function()
            local dir = fresh_tempdir("type-any")
            fs.create_dir_all(dir .. "/sub")
            fs.write(dir .. "/file", "")
            local r = glob.scan("*", { cwd = dir, type = "any" })
            expect(#r).toBe(2)
        end)

        test("type = file is the default", function()
            local dir = fresh_tempdir("type-default")
            fs.create_dir_all(dir .. "/sub")
            fs.write(dir .. "/file", "")
            local r = glob.scan("*", { cwd = dir })
            expect(#r).toBe(1)
            expect(r[1]).toBe("file")
        end)

        test("invalid type returns (nil, err)", function()
            local r, err = glob.scan("*", { type = "socket" })
            expect(r).toBeNil()
            expect(err).toContain("invalid type")
        end)

        test("absolute returns absolute paths", function()
            local dir = fresh_tempdir("absolute")
            fs.write(dir .. "/file.txt", "")
            local r = glob.scan("*.txt", { cwd = dir, absolute = true })
            expect(#r).toBe(1)
            expect(string.sub(r[1], 1, 1)).toBe("/")
            expect(string.find(r[1], "file%.txt$") ~= nil).toBeTruthy()
        end)

        test("round-trip: every result matches the pattern", function()
            local dir = fresh_tempdir("round-trip")
            fs.create_dir_all(dir .. "/src")
            fs.write(dir .. "/src/main.rs", "")
            fs.write(dir .. "/src/lib.rs", "")
            fs.write(dir .. "/README.md", "")
            local pat = "**/*.rs"
            local r = glob.scan(pat, { cwd = dir })
            expect(#r).toBe(2)
            for _, p in ipairs(r) do
                expect(glob.is_match(pat, p)).toBeTruthy()
            end
        end)

        test("invalid pattern returns (nil, err)", function()
            local r, err = glob.scan("src/[*.rs")
            expect(r).toBeNil()
            expect(err).toContain("glob.scan:")
        end)
    end)

    -- ── UserData :scan mirrors flat :scan ────────────────────────────────────

    describe("Glob:scan", function()
        test("scan via compiled handle returns same results as flat scan", function()
            local dir = fresh_tempdir("userdata-scan")
            fs.create_dir_all(dir .. "/src")
            fs.write(dir .. "/src/a.rs", "")
            fs.write(dir .. "/src/b.rs", "")
            fs.write(dir .. "/src/c.md", "")

            local flat = glob.scan("**/*.rs", { cwd = dir })
            local handle = glob.Glob.new("**/*.rs"):scan({ cwd = dir })

            expect(#flat).toBe(#handle)
            for i, p in ipairs(flat) do
                expect(handle[i]).toBe(p)
            end
        end)

        test("scan honours ignore option", function()
            local dir = fresh_tempdir("userdata-scan-ignore")
            fs.create_dir_all(dir .. "/keep")
            fs.create_dir_all(dir .. "/skip")
            fs.write(dir .. "/keep/a", "")
            fs.write(dir .. "/skip/b", "")
            local r = glob.Glob.new("**/*"):scan({
                cwd    = dir,
                ignore = { "skip/**" },
            })
            expect(#r).toBe(1)
            expect(r[1]).toBe("keep/a")
        end)
    end)

    -- ── Invariants ───────────────────────────────────────────────────────────

    describe("invariants", function()
        test("Glob.new(p):is_match == glob.is_match(p, x)", function()
            local cases = {
                { "*.rs", "foo.rs" },
                { "*.rs", "foo.md" },
                { "src/**/*.rs", "src/lua/fs.rs" },
                { "src/**/*.rs", "README.md" },
                { ".git*", ".gitignore" },
                { "*", ".hidden" },
            }
            for _, c in ipairs(cases) do
                local p, x = c[1], c[2]
                expect(glob.Glob.new(p):is_match(x)).toBe(glob.is_match(p, x))
            end
        end)

        test("Glob.new(p):is_match == GlobSet.new({p}):is_match", function()
            local cases = {
                { "*.rs", "foo.rs" },
                { "*.rs", "foo.md" },
                { ".git*", ".gitignore" },
                { "*", ".hidden" },
            }
            for _, c in ipairs(cases) do
                local p, x = c[1], c[2]
                expect(glob.Glob.new(p):is_match(x)).toBe(
                    glob.GlobSet.new({ p }):is_match(x)
                )
            end
        end)
    end)

end)
