local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local hex = require("lib:hex")


describe("hex", function()

    -- encode ----------------------------------------------------------------

    describe("encode", function()
        test("known value", function()
            expect(hex.encode("hi")).toBe("6869")
        end)

        test("empty string", function()
            expect(hex.encode("")).toBe("")
        end)

        test("lowercase alphabet (a-f, never A-F)", function()
            -- 0xff 0xab exercises the letter range.
            expect(hex.encode("\255\171")).toBe("ffab")
        end)

        test("binary input round-trips via decode", function()
            local raw = "\0\1\2\254\255"
            local dec, err = hex.decode(hex.encode(raw))
            expect(err).toBeNil()
            expect(dec).toBe(raw)
        end)
    end)

    -- encode_upper ----------------------------------------------------------

    describe("encode_upper", function()
        test("known value", function()
            expect(hex.encode_upper("\255\171")).toBe("FFAB")
        end)

        test("empty string", function()
            expect(hex.encode_upper("")).toBe("")
        end)

        test("same bytes as encode, upper-cased", function()
            local raw = "\0\1\2\254\255"
            expect(hex.encode_upper(raw)).toBe(hex.encode(raw):upper())
        end)
    end)

    -- decode ----------------------------------------------------------------

    describe("decode", function()
        test("known value", function()
            local result, err = hex.decode("6869")
            expect(err).toBeNil()
            expect(result).toBe("hi")
        end)

        test("empty string decodes to empty bytes", function()
            local result, err = hex.decode("")
            expect(err).toBeNil()
            expect(result).toBe("")
        end)

        test("accepts uppercase", function()
            local result, err = hex.decode("FFAB")
            expect(err).toBeNil()
            expect(result).toBe("\255\171")
        end)

        test("accepts mixed case", function()
            local result, err = hex.decode("fFaB")
            expect(err).toBeNil()
            expect(result).toBe("\255\171")
        end)

        test("tolerates whitespace (spaces, tabs, newlines)", function()
            local result, err = hex.decode("de ad\tbe\nef")
            expect(err).toBeNil()
            expect(result).toBe("\222\173\190\239")
        end)

        test("rejects odd length", function()
            local val, err = hex.decode("abc")
            expect(val).toBeNil()
            expect(err).toContain("hex.decode:")
        end)

        test("rejects non-hex characters", function()
            local val, err = hex.decode("zz")
            expect(val).toBeNil()
            expect(err).toContain("hex.decode:")
        end)
    end)

end)
