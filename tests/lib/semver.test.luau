local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local semver = require("lib:semver")

describe("semver", function()

    -- ── Version UserData ─────────────────────────────────────────────────────

    describe("Version", function()
        test("parse exposes fields", function()
            local v = semver.Version.parse("1.2.3-beta.1+build.42")
            expect(v.major).toBe(1)
            expect(v.minor).toBe(2)
            expect(v.patch).toBe(3)
            expect(v.pre).toBe("beta.1")
            expect(v.build).toBe("build.42")
        end)

        test("pre and build are nil when absent", function()
            local v = semver.Version.parse("1.2.3")
            expect(v.pre).toBeNil()
            expect(v.build).toBeNil()
        end)

        test("parse rejects malformed input", function()
            local v, err = semver.Version.parse("1.2")
            expect(v).toBeNil()
            expect(err).toContain("semver.Version.parse:")
        end)

        test("tostring is canonical", function()
            expect(tostring(semver.Version.parse("1.2.3-rc.1+m"))).toBe("1.2.3-rc.1+m")
        end)

        test("compare accepts string or Version", function()
            local v = semver.Version.parse("1.2.3")
            expect(v:compare("1.2.3")).toBe(0)
            expect(v:compare("1.2.4")).toBe(-1)
            expect(v:compare("1.0.0")).toBe(1)
            expect(v:compare(semver.Version.parse("1.0.0"))).toBe(1)
        end)

        test("metamethods enable comparison", function()
            local a = semver.Version.parse("1.0.0")
            local b = semver.Version.parse("2.0.0")
            local c = semver.Version.parse("1.0.0")
            expect(a < b).toBeTruthy()
            expect(b < a).toBeFalsy()
            expect(a <= c).toBeTruthy()
            expect(a == c).toBeTruthy()
            expect(a == b).toBeFalsy()
        end)

        test("table.sort uses __lt (no comparator)", function()
            local list = {
                semver.Version.parse("2.0.0"),
                semver.Version.parse("1.0.0"),
                semver.Version.parse("1.0.1-rc.1"),
                semver.Version.parse("1.0.1"),
            }
            table.sort(list)
            expect(tostring(list[1])).toBe("1.0.0")
            expect(tostring(list[2])).toBe("1.0.1-rc.1")
            expect(tostring(list[3])).toBe("1.0.1")
            expect(tostring(list[4])).toBe("2.0.0")
        end)

        test("increment returns a new Version (immutable)", function()
            local v = semver.Version.parse("1.2.3")
            local n = v:increment("minor")
            expect(tostring(v)).toBe("1.2.3")
            expect(tostring(n)).toBe("1.3.0")
        end)
    end)

    -- ── Pre-release ordering (SemVer §11) ────────────────────────────────────

    describe("pre-release ordering", function()
        test("full chain", function()
            local chain = {
                "1.0.0-alpha",
                "1.0.0-alpha.1",
                "1.0.0-alpha.beta",
                "1.0.0-beta",
                "1.0.0-beta.2",
                "1.0.0-beta.11",
                "1.0.0-rc.1",
                "1.0.0",
            }
            for i = 1, #chain - 1 do
                local cmp = semver.compare(chain[i], chain[i + 1])
                expect(cmp).toBe(-1)
            end
        end)
    end)

    -- ── Build metadata ignored in precedence (SemVer §10) ────────────────────

    describe("build metadata", function()
        test("equal precedence regardless of build", function()
            local a = semver.Version.parse("1.0.0+a")
            local b = semver.Version.parse("1.0.0+b")
            expect(a == b).toBeTruthy()
            expect(semver.compare(a, b)).toBe(0)
            expect(a < b).toBeFalsy()
        end)
    end)

    -- ── VersionReq UserData ──────────────────────────────────────────────────

    describe("VersionReq", function()
        test("parse + matches", function()
            local r = semver.VersionReq.parse(">=1.0.0, <2.0.0")
            expect(r:matches("1.0.0")).toBeTruthy()
            expect(r:matches("1.5.0")).toBeTruthy()
            expect(r:matches("2.0.0")).toBeFalsy()
            expect(r:matches("0.9.9")).toBeFalsy()
        end)

        test("accepts Version object", function()
            local r = semver.VersionReq.parse("^1.0.0")
            expect(r:matches(semver.Version.parse("1.4.2"))).toBeTruthy()
        end)

        test("rejects malformed range", function()
            local r, err = semver.VersionReq.parse("not-a-range")
            expect(r).toBeNil()
            expect(err).toContain("semver.VersionReq.parse:")
        end)
    end)

    -- ── Flat: parse / valid ──────────────────────────────────────────────────

    describe("parse / valid", function()
        test("parse is an alias for Version.parse", function()
            local v = semver.parse("1.2.3")
            expect(v.major).toBe(1)
            expect(tostring(v)).toBe("1.2.3")
        end)

        test("valid is a boolean predicate", function()
            expect(semver.valid("1.2.3")).toBeTruthy()
            expect(semver.valid("not.a.ver")).toBeFalsy()
            expect(semver.valid("1.2")).toBeFalsy()
        end)
    end)

    -- ── Flat: coerce ─────────────────────────────────────────────────────────

    describe("coerce", function()
        test("test vectors", function()
            local cases = {
                { "v1.2.3",        "1.2.3"     },
                { "V1.2.3",        "1.2.3"     },
                { "1.2",           "1.2.0"     },
                { "1",             "1.0.0"     },
                { "release-1.2.3", "1.2.3"     },
                { "v1.2.3-rc1",    "1.2.3-rc1" },
                { "1.2.3.4",       "1.2.3"     },
                { "1.2.3+build",   "1.2.3"     },
            }
            for _, c in ipairs(cases) do
                local got = semver.coerce(c[1])
                expect(tostring(got)).toBe(c[2])
            end
        end)

        test("uncoercible input returns (nil, err)", function()
            for _, bad in ipairs({ "abc", "", "  " }) do
                local v, err = semver.coerce(bad)
                expect(v).toBeNil()
                expect(err).toContain("semver.coerce:")
            end
        end)
    end)

    -- ── Flat: compare ────────────────────────────────────────────────────────

    describe("compare", function()
        test("strings", function()
            expect(semver.compare("1.2.3", "1.2.3")).toBe(0)
            expect(semver.compare("1.2.3", "1.2.4")).toBe(-1)
            expect(semver.compare("1.2.4", "1.2.3")).toBe(1)
        end)

        test("identity: compare(a, a) == 0", function()
            for _, s in ipairs({ "0.0.0", "1.2.3", "1.0.0-rc.1", "10.20.30+build" }) do
                expect(semver.compare(s, s)).toBe(0)
            end
        end)

        test("symmetry: compare(a, b) == -compare(b, a)", function()
            local pairs_ = {
                { "1.0.0", "2.0.0" },
                { "1.0.0", "1.0.1" },
                { "1.0.0-rc.1", "1.0.0" },
            }
            for _, p in ipairs(pairs_) do
                expect(semver.compare(p[1], p[2])).toBe(-semver.compare(p[2], p[1]))
            end
        end)

        test("mixed input produces identical results", function()
            local v = semver.Version.parse("1.2.3")
            expect(semver.compare(v, "1.2.3")).toBe(0)
            expect(semver.compare("1.2.3", v)).toBe(0)
            expect(semver.compare(v, "1.2.4")).toBe(-1)
            expect(semver.compare("1.2.4", v)).toBe(1)
        end)
    end)

    -- ── Flat: satisfies ──────────────────────────────────────────────────────

    describe("satisfies", function()
        test("basic ranges", function()
            expect(semver.satisfies("1.2.3", "^1.0.0")).toBeTruthy()
            expect(semver.satisfies("2.0.0", "^1.0.0")).toBeFalsy()
            expect(semver.satisfies("1.2.3", "~1.2.0")).toBeTruthy()
            expect(semver.satisfies("1.3.0", "~1.2.0")).toBeFalsy()
        end)

        test("pre-release excluded from plain range", function()
            expect(semver.satisfies("1.0.0-alpha", "^1.0.0")).toBeFalsy()
        end)

        test("Cargo zero-major caret is exact", function()
            expect(semver.satisfies("0.0.1", "^0.0.1")).toBeTruthy()
            expect(semver.satisfies("0.0.2", "^0.0.1")).toBeFalsy()
        end)

        test("invalid range returns (nil, err) not false", function()
            local ok, err = semver.satisfies("1.0.0", "<<bad>>")
            expect(ok).toBeNil()
            expect(err).toContain("semver.satisfies:")
        end)

        test("accepts Version + VersionReq objects", function()
            local v = semver.Version.parse("1.5.0")
            local r = semver.VersionReq.parse(">=1.0.0, <2.0.0")
            expect(semver.satisfies(v, r)).toBeTruthy()
            expect(semver.satisfies(v, "^1.0.0")).toBeTruthy()
            expect(semver.satisfies("1.5.0", r)).toBeTruthy()
        end)
    end)

    -- ── Flat: increment ──────────────────────────────────────────────────────

    describe("increment", function()
        test("level vectors", function()
            local cases = {
                { "0.0.0", "major",      "1.0.0"        },
                { "1.2.3", "minor",      "1.3.0"        },
                { "1.2.3", "patch",      "1.2.4"        },
                { "1.2.3-beta.1", "patch", "1.2.3"      },
                { "1.2.3", "prerelease", "1.2.4-0"      },
                { "1.2.3-beta", "prerelease", "1.2.3-beta.0" },
                { "1.2.3-beta.1", "prerelease", "1.2.3-beta.2" },
            }
            for _, c in ipairs(cases) do
                expect(semver.increment(c[1], c[2])).toBe(c[3])
            end
        end)

        test("build metadata stripped from result", function()
            expect(semver.increment("1.2.3+meta", "minor")).toBe("1.3.0")
            expect(semver.increment("1.2.3+meta", "patch")).toBe("1.2.4")
        end)

        test("invalid level returns (nil, err)", function()
            local s, err = semver.increment("1.2.3", "bogus")
            expect(s).toBeNil()
            expect(err).toContain("invalid level")
        end)

        test("invalid version returns (nil, err)", function()
            local s, err = semver.increment("not.a.ver", "patch")
            expect(s).toBeNil()
            expect(err).toContain("semver.increment:")
        end)
    end)

    -- ── Flat: format ─────────────────────────────────────────────────────────

    describe("format", function()
        test("equivalent to tostring", function()
            local v = semver.Version.parse("1.2.3-rc.1+meta")
            expect(semver.format(v)).toBe(tostring(v))
            expect(semver.format(v)).toBe("1.2.3-rc.1+meta")
        end)
    end)

end)
