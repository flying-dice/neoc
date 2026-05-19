local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local regex = require("lib:regex")

describe("regex", function()

    -- ── Flat: is_match ───────────────────────────────────────────────────────

    describe("is_match", function()
        test("digits in mixed text", function()
            expect(regex.is_match("\\d+", "abc 123 def")).toBeTruthy()
            expect(regex.is_match("\\d+", "abc def")).toBeFalsy()
        end)

        test("anchored alternation", function()
            expect(regex.is_match("^(yes|no)$", "yes")).toBeTruthy()
            expect(regex.is_match("^(yes|no)$", "yesno")).toBeFalsy()
        end)

        test("invalid pattern returns (nil, err)", function()
            local ok, err = regex.is_match("(abc", "anything")
            expect(ok).toBeNil()
            expect(err).toContain("regex.is_match:")
        end)
    end)

    -- ── Flat: find / find_all ────────────────────────────────────────────────

    describe("find / find_all", function()
        test("find returns text + 1-based byte positions", function()
            local m = regex.find("\\d+", "abc 123 def")
            expect(m.text).toBe("123")
            expect(m.start).toBe(5)
            expect(m.finish).toBe(7)
        end)

        test("positions agree with string.find for a literal", function()
            local s = "abc 123 def 456"
            local s1, e1 = string.find(s, "123", 1, true)
            local m = regex.find("123", s)
            expect(m.start).toBe(s1)
            expect(m.finish).toBe(e1)
        end)

        test("find on no match returns nil", function()
            local m = regex.find("zzz", "abc")
            expect(m).toBeNil()
        end)

        test("find_all returns every match in order", function()
            local out = {}
            for i, m in ipairs(regex.find_all("\\d+", "1 22 333 4444")) do
                out[i] = m.text
            end
            expect(out[1]).toBe("1")
            expect(out[2]).toBe("22")
            expect(out[3]).toBe("333")
            expect(out[4]).toBe("4444")
        end)

        test("find_all on no match returns empty table", function()
            local out = regex.find_all("\\d+", "abc")
            expect(#out).toBe(0)
        end)

        test("positional groups are 1-based", function()
            local m = regex.find("(\\d+)-(\\d+)", "size 12-34 px")
            expect(m.text).toBe("12-34")
            expect(m.groups[1]).toBe("12")
            expect(m.groups[2]).toBe("34")
        end)

        test("unmatched optional group is nil", function()
            local m = regex.find("(a)(b)?", "a")
            expect(m.groups[1]).toBe("a")
            expect(m.groups[2]).toBeNil()
        end)

        test("named groups", function()
            local m = regex.find(
                "(?P<y>\\d{4})-(?P<m>\\d{2})-(?P<d>\\d{2})",
                "date 2026-05-19 done"
            )
            expect(m.named.y).toBe("2026")
            expect(m.named.m).toBe("05")
            expect(m.named.d).toBe("19")
        end)
    end)

    -- ── Flags ────────────────────────────────────────────────────────────────

    describe("flags", function()
        test("i — case-insensitive", function()
            expect(regex.is_match("hello", "Hello", "i")).toBeTruthy()
            expect(regex.is_match("hello", "Hello")).toBeFalsy()
        end)

        test("m — multi-line anchors", function()
            local s = "first\nsecond"
            expect(regex.is_match("^second$", s)).toBeFalsy()
            expect(regex.is_match("^second$", s, "m")).toBeTruthy()
        end)

        test("s — dot matches newline", function()
            expect(regex.is_match("a.b", "a\nb")).toBeFalsy()
            expect(regex.is_match("a.b", "a\nb", "s")).toBeTruthy()
        end)

        test("inline (?i) equivalent to flag argument", function()
            expect(regex.is_match("(?i)hello", "Hello")).toBeTruthy()
        end)

        test("unknown flag rejected with listing", function()
            local ok, err = regex.is_match("a", "a", "z")
            expect(ok).toBeNil()
            expect(err).toContain("unknown flag")
            expect(err).toContain("i, m, s, x")
        end)
    end)

    -- ── replace / replace_all ────────────────────────────────────────────────

    describe("replace / replace_all", function()
        test("replace targets the first match only", function()
            expect(regex.replace("a", "aaa", "X")).toBe("Xaa")
        end)

        test("replace_all hits every match", function()
            expect(regex.replace_all("a", "aaa", "X")).toBe("XXX")
        end)

        test("$1 / $2 capture references", function()
            expect(regex.replace_all("(\\w+)=(\\w+)", "a=1 b=2", "$2=$1"))
                .toBe("1=a 2=b")
        end)

        test("${name} named-capture reference", function()
            expect(regex.replace_all("(?P<k>\\w+)=(?P<v>\\w+)", "x=10", "${v}=${k}"))
                .toBe("10=x")
        end)
    end)

    -- ── split ────────────────────────────────────────────────────────────────

    describe("split", function()
        test("whitespace runs", function()
            local parts = regex.split("\\s+", "one  two   three")
            expect(parts[1]).toBe("one")
            expect(parts[2]).toBe("two")
            expect(parts[3]).toBe("three")
        end)

        test("limit caps part count, last keeps remainder", function()
            local parts = regex.split(",", "a,b,c,d", 2)
            expect(parts[1]).toBe("a")
            expect(parts[2]).toBe("b,c,d")
            expect(#parts).toBe(2)
        end)

        test("limit = 0 is unlimited", function()
            local parts = regex.split(",", "a,b,c", 0)
            expect(#parts).toBe(3)
        end)

        test("negative limit returns (nil, err)", function()
            local parts, err = regex.split(",", "a,b", -1)
            expect(parts).toBeNil()
            expect(err).toContain("limit must be >= 0")
        end)
    end)

    -- ── escape ───────────────────────────────────────────────────────────────

    describe("escape", function()
        test("round-trip via is_match", function()
            local raw = "1.2.3+build (rc)"
            local pat = regex.escape(raw)
            expect(regex.is_match(pat, raw)).toBeTruthy()
        end)
    end)

    -- ── Pattern UserData ─────────────────────────────────────────────────────

    describe("Pattern", function()
        test("new + is_match + find_all reusable", function()
            local p = regex.Pattern.new("\\d+")
            expect(p:is_match("1 2 3")).toBeTruthy()
            expect(#p:find_all("1 22 333")).toBe(3)
            expect(p:is_match("nope")).toBeFalsy()
        end)

        test("tostring returns the source pattern", function()
            expect(tostring(regex.Pattern.new("\\d+"))).toBe("\\d+")
        end)

        test("invalid pattern returns (nil, err)", function()
            local p, err = regex.Pattern.new("(abc")
            expect(p).toBeNil()
            expect(err).toContain("regex.Pattern.new:")
        end)

        test("flags forwarded at compile time", function()
            local p = regex.Pattern.new("hello", "i")
            expect(p:is_match("HELLO")).toBeTruthy()
        end)

        test("replace / replace_all on Pattern", function()
            local p = regex.Pattern.new("a")
            expect(p:replace("aaa", "X")).toBe("Xaa")
            expect(p:replace_all("aaa", "X")).toBe("XXX")
        end)

        test("split on Pattern with limit", function()
            local p = regex.Pattern.new(",")
            local parts = p:split("a,b,c,d", 2)
            expect(parts[1]).toBe("a")
            expect(parts[2]).toBe("b,c,d")
        end)

        test("split error carries module-scoped prefix", function()
            local parts, err = regex.Pattern.new(","):split("a,b", -1)
            expect(parts).toBeNil()
            expect(err).toContain("regex.Pattern.split:")
        end)

        test("find returns a single value (infallible)", function()
            local p = regex.Pattern.new("\\d+")
            expect(select("#", p:find("abc 123"))).toBe(1)
            expect(select("#", p:find("abc"))).toBe(1)
        end)
    end)

    -- ── Unicode ──────────────────────────────────────────────────────────────

    describe("unicode", function()
        test("\\w matches non-ASCII letters by default", function()
            expect(regex.is_match("^\\w+$", "café")).toBeTruthy()
        end)

        test("positions are byte offsets, not char offsets", function()
            -- "caf" = 3 bytes, 'é' = 2 bytes starting at byte 4.
            local m = regex.find("é", "café X")
            expect(m.start).toBe(4)
            expect(m.finish).toBe(5)
        end)
    end)

    -- ── Round-trip invariant ────────────────────────────────────────────────

    describe("round-trip", function()
        test("every find_all hit is also is_match", function()
            local pat = "\\w{3,}"
            for _, m in ipairs(regex.find_all(pat, "the quick brown fox")) do
                expect(regex.is_match(pat, m.text)).toBeTruthy()
            end
        end)
    end)
end)
