local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local rng = require("lib:random")

describe("random", function()

    -- random_bytes ----------------------------------------------------------

    describe("random_bytes", function()
        test("returns requested length", function()
            local b = rng.random_bytes(32)
            expect(#b).toBe(32)
        end)

        test("zero length is empty string", function()
            local b = rng.random_bytes(0)
            expect(b).toBe("")
        end)

        test("two draws differ", function()
            -- 32 bytes — collision probability negligible.
            local a = rng.random_bytes(32)
            local b = rng.random_bytes(32)
            expect(a == b).toBe(false)
        end)

        test("negative count returns (nil, err)", function()
            local v, err = rng.random_bytes(-1)
            expect(v).toBeNil()
            expect(err).toContain("random.random_bytes:")
        end)
    end)

    -- random_int ------------------------------------------------------------

    describe("random_int", function()
        test("respects inclusive bounds", function()
            for _ = 1, 200 do
                local v = rng.random_int(1, 6)
                expect(v >= 1 and v <= 6).toBe(true)
            end
        end)

        test("width 1 returns the bound", function()
            local v = rng.random_int(42, 42)
            expect(v).toBe(42)
        end)

        test("supports negative ranges", function()
            for _ = 1, 100 do
                local v = rng.random_int(-5, -1)
                expect(v >= -5 and v <= -1).toBe(true)
            end
        end)

        test("min > max returns (nil, err)", function()
            local v, err = rng.random_int(10, 5)
            expect(v).toBeNil()
            expect(err).toContain("random.random_int:")
        end)
    end)

    -- random_string ---------------------------------------------------------

    describe("random_string", function()
        test("default alphabet length and charset", function()
            local s = rng.random_string(24)
            expect(#s).toBe(24)
            expect(s:match("^[%w]+$") ~= nil).toBe(true)
        end)

        test("custom alphabet restricts output", function()
            local s = rng.random_string(50, "01")
            expect(#s).toBe(50)
            expect(s:match("^[01]+$") ~= nil).toBe(true)
        end)

        test("zero length is empty string", function()
            local s = rng.random_string(0)
            expect(s).toBe("")
        end)

        test("empty alphabet returns (nil, err)", function()
            local v, err = rng.random_string(8, "")
            expect(v).toBeNil()
            expect(err).toContain("random.random_string:")
        end)

        test("negative length returns (nil, err)", function()
            local v, err = rng.random_string(-1)
            expect(v).toBeNil()
            expect(err).toContain("random.random_string:")
        end)
    end)

    -- token_urlsafe ---------------------------------------------------------

    describe("token_urlsafe", function()
        test("uses url-safe alphabet without padding", function()
            local s = rng.token_urlsafe(32)
            expect(s:match("^[A-Za-z0-9_%-]+$") ~= nil).toBe(true)
            expect(s:find("=") == nil).toBe(true)
        end)

        test("32-byte input encodes to 43 chars", function()
            -- ceil(32 * 4 / 3) = 43, no padding.
            local s = rng.token_urlsafe(32)
            expect(#s).toBe(43)
        end)

        test("zero bytes is empty string", function()
            local s = rng.token_urlsafe(0)
            expect(s).toBe("")
        end)

        test("negative count returns (nil, err)", function()
            local v, err = rng.token_urlsafe(-1)
            expect(v).toBeNil()
            expect(err).toContain("random.token_urlsafe:")
        end)
    end)

    -- uuid_v4 ---------------------------------------------------------------

    describe("uuid_v4", function()
        test("returns 8-4-4-4-12 canonical form", function()
            local s = rng.uuid_v4()
            expect(#s).toBe(36)
            expect(s:match("^%x%x%x%x%x%x%x%x%-%x%x%x%x%-4%x%x%x%-[89ab]%x%x%x%-%x%x%x%x%x%x%x%x%x%x%x%x$") ~= nil).toBe(true)
        end)

        test("two draws differ", function()
            local a = rng.uuid_v4()
            local b = rng.uuid_v4()
            expect(a == b).toBe(false)
        end)
    end)

    -- uuid_v7 ---------------------------------------------------------------

    describe("uuid_v7", function()
        test("returns canonical form with v7 marker", function()
            local s = rng.uuid_v7()
            expect(#s).toBe(36)
            -- Version is the first hex digit of group 3; group 3 starts at
            -- position 15 (1-indexed) in the canonical form
            -- xxxxxxxx-xxxx-Mxxx-Nxxx-xxxxxxxxxxxx.
            expect(s:sub(15, 15)).toBe("7")
        end)
    end)

    -- uuid_parse / uuid_format ---------------------------------------------

    describe("uuid_parse / uuid_format", function()
        test("round-trips canonical → bytes → canonical", function()
            local u = rng.uuid_v4()
            local bytes, err = rng.uuid_parse(u)
            expect(err).toBeNil()
            expect(#bytes).toBe(16)
            local back = rng.uuid_format(bytes)
            expect(back).toBe(u)
        end)

        test("accepts uppercase input and normalises to lowercase", function()
            local bytes, err = rng.uuid_parse("550E8400-E29B-41D4-A716-446655440000")
            expect(err).toBeNil()
            expect(rng.uuid_format(bytes)).toBe("550e8400-e29b-41d4-a716-446655440000")
        end)

        test("invalid input returns (nil, err)", function()
            local v, err = rng.uuid_parse("not-a-uuid")
            expect(v).toBeNil()
            expect(err).toContain("random.uuid_parse:")
        end)

        test("uuid_format rejects wrong length", function()
            local v, err = rng.uuid_format("too short")
            expect(v).toBeNil()
            expect(err).toContain("random.uuid_format:")
        end)
    end)

end)
