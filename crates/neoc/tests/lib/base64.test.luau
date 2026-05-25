local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local base64 = require("lib:base64")


describe("base64", function()

    -- encode ----------------------------------------------------------------

    describe("encode", function()
        test("known value", function()
            local result = base64.encode("hello")
            expect(result).toBe("aGVsbG8=")
        end)

        test("empty string", function()
            local result = base64.encode("")
            expect(result).toBe("")
        end)

        test("produces padding", function()
            local result = base64.encode("hi")
            expect(result).toBe("aGk=")
        end)

        test("binary input round-trips via decode", function()
            local raw = "\0\1\2\254\255"
            local enc = base64.encode(raw)
            local dec, err = base64.decode(enc)
            expect(err).toBeNil()
            expect(dec).toBe(raw)
        end)
    end)

    -- decode ----------------------------------------------------------------

    describe("decode", function()
        test("known value", function()
            local result, err = base64.decode("aGVsbG8=")
            expect(err).toBeNil()
            expect(result).toBe("hello")
        end)

        test("invalid input returns (nil, err)", function()
            local val, err = base64.decode("not*base64!")
            expect(val).toBeNil()
            expect(err).toContain("base64.decode:")
        end)
    end)

    -- encode_no_pad ---------------------------------------------------------

    describe("encode_no_pad", function()
        test("omits trailing =", function()
            local result = base64.encode_no_pad("hi")
            expect(result).toBe("aGk")
        end)

        test("empty string", function()
            local result = base64.encode_no_pad("")
            expect(result).toBe("")
        end)

        test("uses standard alphabet (no - or _)", function()
            local raw = "\251\255\254"
            local result = base64.encode_no_pad(raw)
            local has_dash  = result:find("-", 1, true) ~= nil
            local has_under = result:find("_", 1, true) ~= nil
            expect(has_dash).toBeFalsy()
            expect(has_under).toBeFalsy()
        end)
    end)

    -- decode_no_pad ---------------------------------------------------------

    describe("decode_no_pad", function()
        test("decodes unpadded input", function()
            local result, err = base64.decode_no_pad("aGk")
            expect(err).toBeNil()
            expect(result).toBe("hi")
        end)

        test("rejects padded input", function()
            local val, err = base64.decode_no_pad("aGVsbG8=")
            expect(val).toBeNil()
            expect(err).toContain("base64.decode_no_pad:")
        end)

        test("invalid input returns (nil, err)", function()
            local val, err = base64.decode_no_pad("not*base64!")
            expect(val).toBeNil()
            expect(err).toContain("base64.decode_no_pad:")
        end)
    end)

    -- encode_url_safe -------------------------------------------------------

    describe("encode_url_safe", function()
        test("replaces + and / with - and _", function()
            local raw = "\251\255\254"
            local std_enc = base64.encode(raw)
            local url_enc = base64.encode_url_safe(raw)
            expect(std_enc).toContain("+")
            local has_plus  = url_enc:find("+", 1, true) ~= nil
            local has_slash = url_enc:find("/", 1, true) ~= nil
            expect(has_plus).toBeFalsy()
            expect(has_slash).toBeFalsy()
            expect(#url_enc).toBe(#std_enc)
        end)

        test("empty string", function()
            local result = base64.encode_url_safe("")
            expect(result).toBe("")
        end)

        test("produces padding", function()
            local result = base64.encode_url_safe("hi")
            expect(result).toBe("aGk=")
        end)
    end)

    -- decode_url_safe -------------------------------------------------------

    describe("decode_url_safe", function()
        test("round-trips url-safe encoded bytes", function()
            local raw = "\251\255\254"
            local enc = base64.encode_url_safe(raw)
            local dec, err = base64.decode_url_safe(enc)
            expect(err).toBeNil()
            expect(dec).toBe(raw)
        end)

        test("invalid input returns (nil, err)", function()
            local val, err = base64.decode_url_safe("not*base64!")
            expect(val).toBeNil()
            expect(err).toContain("base64.decode_url_safe:")
        end)
    end)

    -- encode_url_safe_no_pad ------------------------------------------------

    describe("encode_url_safe_no_pad", function()
        test("no padding and url-safe alphabet", function()
            local raw = "\251\255\254"
            local result = base64.encode_url_safe_no_pad(raw)
            local has_eq    = result:find("=", 1, true) ~= nil
            local has_plus  = result:find("+", 1, true) ~= nil
            local has_slash = result:find("/", 1, true) ~= nil
            expect(has_eq).toBeFalsy()
            expect(has_plus).toBeFalsy()
            expect(has_slash).toBeFalsy()
        end)

        test("empty string", function()
            local result = base64.encode_url_safe_no_pad("")
            expect(result).toBe("")
        end)

        test("shorter than padded equivalent", function()
            local padded = base64.encode_url_safe("hi")
            local bare   = base64.encode_url_safe_no_pad("hi")
            expect(#bare < #padded).toBeTruthy()
        end)
    end)

    -- decode_url_safe_no_pad ------------------------------------------------

    describe("decode_url_safe_no_pad", function()
        test("round-trips url-safe no-pad encoded bytes", function()
            local raw = "\251\255\254"
            local enc = base64.encode_url_safe_no_pad(raw)
            local dec, err = base64.decode_url_safe_no_pad(enc)
            expect(err).toBeNil()
            expect(dec).toBe(raw)
        end)

        test("rejects padded input", function()
            local val, err = base64.decode_url_safe_no_pad("aGVsbG8=")
            expect(val).toBeNil()
            expect(err).toContain("base64.decode_url_safe_no_pad:")
        end)

        test("invalid input returns (nil, err)", function()
            local val, err = base64.decode_url_safe_no_pad("not*base64!")
            expect(val).toBeNil()
            expect(err).toContain("base64.decode_url_safe_no_pad:")
        end)
    end)

end)
