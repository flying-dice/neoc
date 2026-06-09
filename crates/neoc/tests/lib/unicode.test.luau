local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local uni = require("lib:unicode")

describe("unicode", function()

    -- normalize --------------------------------------------------------------

    describe("normalize", function()
        test("composes to NFC", function()
            -- "e" + U+0301 COMBINING ACUTE ACCENT composes to U+00E9 "é".
            local s, err = uni.normalize("e\u{0301}", "nfc")
            expect(err).toBeNil()
            expect(s).toBe("\u{00E9}")
        end)

        test("decomposes to NFD", function()
            -- Precomposed "é" (2 bytes) decomposes to "e" + accent (3 bytes).
            local s, err = uni.normalize("\u{00E9}", "nfd")
            expect(err).toBeNil()
            expect(#s).toBe(3)
            expect(uni.grapheme_count(s)).toBe(1)
        end)

        test("NFKC folds compatibility ligature", function()
            expect(uni.normalize("\u{FB01}", "nfkc")).toBe("fi")  -- ﬁ → fi
        end)

        test("NFKD folds compatibility digit", function()
            expect(uni.normalize("\u{2460}", "nfkd")).toBe("1")  -- ① → 1
        end)

        test("form name is case-insensitive", function()
            local s, err = uni.normalize("\u{00E9}", "NFC")
            expect(err).toBeNil()
            expect(s).toBe("\u{00E9}")
        end)

        test("NFC is idempotent", function()
            local once = uni.normalize("e\u{0301}", "nfc")
            expect(uni.normalize(once, "nfc")).toBe(once)
        end)

        test("unknown form errors", function()
            local v, e = uni.normalize("x", "nfz")
            expect(v).toBeNil()
            expect(string.find(e, "unknown form") ~= nil).toBeTruthy()
            expect(string.find(e, "nfz") ~= nil).toBeTruthy()
        end)
    end)

    -- fold -------------------------------------------------------------------

    describe("fold", function()
        test("lowercases ASCII", function()
            expect(uni.fold("HELLO")).toBe("hello")
        end)

        test("sharp s folds to ss", function()
            -- "STRASSE" and "straße" share one case-fold key.
            expect(uni.fold("STRASSE")).toBe(uni.fold("stra\u{00DF}e"))
        end)

        test("capital sharp S folds to ss", function()
            expect(uni.fold("\u{1E9E}")).toBe("ss")  -- ẞ → ss
        end)
    end)

    -- caseless_eq ------------------------------------------------------------

    describe("caseless_eq", function()
        test("ignores case", function()
            expect(uni.caseless_eq("Hello", "hello")).toBeTruthy()
        end)

        test("ignores canonical combining differences", function()
            -- NFC "é" vs NFD "e"+accent, also differing case.
            expect(uni.caseless_eq("\u{00C9}", "e\u{0301}")).toBeTruthy()
        end)

        test("distinct words do not match", function()
            expect(uni.caseless_eq("apple", "orange")).toBeFalsy()
        end)

        test("empty strings match", function()
            expect(uni.caseless_eq("", "")).toBeTruthy()
        end)
    end)

    -- graphemes --------------------------------------------------------------

    describe("graphemes", function()
        test("splits ASCII into single characters", function()
            local r = uni.graphemes("abc")
            expect(#r).toBe(3)
            expect(r[1]).toBe("a")
            expect(r[3]).toBe("c")
        end)

        test("emoji with skin-tone modifier is one cluster", function()
            local r = uni.graphemes("a\u{1F44D}\u{1F3FD}b")  -- a 👍🏽 b
            expect(#r).toBe(3)
            expect(r[1]).toBe("a")
        end)

        test("regional-indicator flag is one cluster", function()
            expect(#uni.graphemes("\u{1F1EC}\u{1F1E7}")).toBe(1)  -- 🇬🇧
        end)

        test("empty string yields empty array", function()
            expect(#uni.graphemes("")).toBe(0)
        end)
    end)

    -- grapheme_count ---------------------------------------------------------

    describe("grapheme_count", function()
        test("counts user-perceived characters", function()
            expect(uni.grapheme_count("a\u{1F44D}\u{1F3FD}b")).toBe(3)
        end)

        test("flag counts as one", function()
            expect(uni.grapheme_count("\u{1F1EC}\u{1F1E7}")).toBe(1)
        end)

        test("empty is zero", function()
            expect(uni.grapheme_count("")).toBe(0)
        end)
    end)

    -- category ---------------------------------------------------------------

    describe("category", function()
        test("classifies single codepoints", function()
            expect(uni.category("A")).toBe("Lu")
            expect(uni.category("a")).toBe("Ll")
            expect(uni.category("\u{01C5}")).toBe("Lt")  -- ǅ titlecase
            expect(uni.category("0")).toBe("Nd")
            expect(uni.category(" ")).toBe("Zs")
            expect(uni.category("_")).toBe("Pc")
            expect(uni.category("$")).toBe("Sc")
        end)

        test("multi-codepoint input errors", function()
            local v, e = uni.category("ab")
            expect(v).toBeNil()
            expect(string.find(e, "single codepoint") ~= nil).toBeTruthy()
        end)

        test("empty input errors", function()
            local v, e = uni.category("")
            expect(v).toBeNil()
            expect(string.find(e, "got 0") ~= nil).toBeTruthy()
        end)
    end)

    -- is_alphabetic ----------------------------------------------------------

    describe("is_alphabetic", function()
        test("ASCII letters are alphabetic", function()
            expect(uni.is_alphabetic("abc")).toBeTruthy()
        end)

        test("accented and CJK letters are alphabetic", function()
            expect(uni.is_alphabetic("h\u{00E9}llo")).toBeTruthy()
            expect(uni.is_alphabetic("\u{65E5}\u{672C}\u{8A9E}")).toBeTruthy()  -- 日本語
        end)

        test("digits make it non-alphabetic", function()
            expect(uni.is_alphabetic("abc1")).toBeFalsy()
        end)

        test("whitespace makes it non-alphabetic", function()
            expect(uni.is_alphabetic("a b")).toBeFalsy()
        end)

        test("empty string is not alphabetic", function()
            expect(uni.is_alphabetic("")).toBeFalsy()
        end)
    end)

    -- is_numeric -------------------------------------------------------------

    describe("is_numeric", function()
        test("ASCII digits are numeric", function()
            expect(uni.is_numeric("123")).toBeTruthy()
        end)

        test("Unicode number letters are numeric", function()
            expect(uni.is_numeric("\u{216B}")).toBeTruthy()  -- Ⅻ roman numeral twelve (Nl)
        end)

        test("letters make it non-numeric", function()
            expect(uni.is_numeric("12a")).toBeFalsy()
        end)

        test("empty string is not numeric", function()
            expect(uni.is_numeric("")).toBeFalsy()
        end)
    end)

    -- width ------------------------------------------------------------------

    describe("width", function()
        test("ASCII is one column per char", function()
            expect(uni.width("abc")).toBe(3)
        end)

        test("fullwidth CJK is two columns each", function()
            expect(uni.width("\u{4E16}\u{754C}")).toBe(4)  -- 世界
        end)

        test("mixed narrow and wide", function()
            expect(uni.width("a\u{4E16}")).toBe(3)  -- a + 世
        end)

        test("combining marks are zero-width", function()
            expect(uni.width("e\u{0301}")).toBe(1)
        end)

        test("empty string is zero width", function()
            expect(uni.width("")).toBe(0)
        end)
    end)

end)
