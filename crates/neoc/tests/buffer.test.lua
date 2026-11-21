-- Luau's built-in `buffer` library, as `neoc run` offers it: a fixed-size, zero-filled, mutable block of bytes,
-- read and written at 0-based offsets, little-endian. It is the engine's own (not a neoc module), so these cases
-- hold what a script relies on: that it is there, that a size is checked and that access is bounded. That a
-- serializer or a thread transfer refuses a buffer is held by each of those modules' own suites ("a buffer").

local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

-- the error a call raised, as text; fails the test when it did not raise
local function raised(f, ...)
    local ok, err = pcall(f, ...)
    expect(ok).toBe(false)
    return tostring(err)
end

describe("buffer", function()

    describe("create", function()
        test("makes a zero-filled buffer of the given size", function()
            local b = buffer.create(4)
            expect(type(b)).toBe("buffer")
            expect(buffer.len(b)).toBe(4)
            expect(buffer.readu32(b, 0)).toBe(0)
        end)

        test("a size of zero is an empty buffer", function()
            expect(buffer.len(buffer.create(0))).toBe(0)
        end)

        test("a negative size is an error", function()
            expect(raised(buffer.create, -1)).toContain("invalid argument #1 to 'create' (size)")
            expect(raised(buffer.create, -1 / 0)).toContain("invalid argument #1 to 'create' (size)")
        end)

        test("a size that is not a number is an error", function()
            expect(raised(buffer.create, "eight")).toContain("number expected, got string")
            expect(raised(buffer.create, {})).toContain("number expected, got table")
            expect(raised(buffer.create)).toContain("missing argument #1 to 'create'")
        end)

        test("a size over Luau's limit (1 GiB) is an error, and nothing is allocated", function()
            expect(raised(buffer.create, 2 ^ 30 + 1)).toContain("block too big")
            expect(raised(buffer.create, 2 ^ 53)).toContain("block too big")
            expect(raised(buffer.create, 1 / 0)).toContain("block too big")
        end)
    end)

    describe("read and write", function()
        test("integers and floats round-trip, little-endian", function()
            local b = buffer.create(16)
            buffer.writeu32(b, 0, 0x01020304)
            expect(buffer.readu8(b, 0)).toBe(4)
            expect(buffer.readu8(b, 3)).toBe(1)
            buffer.writei16(b, 4, -2)
            expect(buffer.readi16(b, 4)).toBe(-2)
            expect(buffer.readu16(b, 4)).toBe(65534)
            buffer.writef64(b, 8, 0.5)
            expect(buffer.readf64(b, 8)).toBe(0.5)
        end)

        test("strings go in and out, and a buffer is mutable where a string is not", function()
            local b = buffer.fromstring("hello")
            buffer.writestring(b, 0, "J")
            expect(buffer.tostring(b)).toBe("Jello")
            expect(buffer.readstring(b, 1, 3)).toBe("ell")
        end)

        test("copy and fill work within one buffer and across two", function()
            local b = buffer.create(6)
            buffer.fill(b, 0, 0x61, 3)
            buffer.copy(b, 3, buffer.fromstring("xyz"))
            expect(buffer.tostring(b)).toBe("aaaxyz")
        end)

        test("an access out of bounds is an error, and the buffer does not grow", function()
            local b = buffer.create(4)
            expect(raised(buffer.readu8, b, 4)).toContain("buffer access out of bounds")
            expect(raised(buffer.readu8, b, -1)).toContain("buffer access out of bounds")
            expect(raised(buffer.writeu32, b, 1, 1)).toContain("buffer access out of bounds")
            expect(buffer.len(b)).toBe(4)
        end)
    end)

end)
