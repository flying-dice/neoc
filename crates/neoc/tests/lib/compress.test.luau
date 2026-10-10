local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local compress = require("lib:compress")


describe("compress", function()

    -- round-trip per format ---------------------------------------------------

    describe("round-trip", function()
        for _, fmt in ipairs({ "gzip", "zlib", "zstd", "brotli" }) do
            test(fmt .. " round-trips", function()
                local original = "hello world"
                local compressed, enc_err = compress.encode(fmt, original)
                expect(enc_err).toBeNil()
                local decompressed, dec_err = compress.decode(fmt, compressed)
                expect(dec_err).toBeNil()
                expect(decompressed).toBe(original)
            end)
        end

        test("binary round-trip", function()
            local raw = "\0\1\2\254\255"
            local compressed = compress.encode("gzip", raw)
            local decompressed = compress.decode("gzip", compressed)
            expect(decompressed).toBe(raw)
        end)

        test("large payload round-trip", function()
            local big = string.rep("abcdefghij", 10000)
            local compressed = compress.encode("zstd", big)
            local decompressed = compress.decode("zstd", compressed)
            expect(decompressed).toBe(big)
        end)

        test("empty round-trip", function()
            for _, fmt in ipairs({ "gzip", "zlib", "zstd", "brotli" }) do
                local compressed, enc_err = compress.encode(fmt, "")
                expect(enc_err).toBeNil()
                local decompressed, dec_err = compress.decode(fmt, compressed)
                expect(dec_err).toBeNil()
                expect(decompressed).toBe("")
            end
        end)
    end)

    -- encode ------------------------------------------------------------------

    describe("encode", function()
        test("empty string produces output", function()
            for _, fmt in ipairs({ "gzip", "zlib", "zstd", "brotli" }) do
                local result, err = compress.encode(fmt, "")
                expect(err).toBeNil()
                expect(#result > 0).toBeTruthy()
            end
        end)

        test("with level option", function()
            local compressed = compress.encode("gzip", "hello", { level = 9 })
            local decompressed = compress.decode("gzip", compressed)
            expect(decompressed).toBe("hello")
        end)

        test("zstd level range", function()
            local compressed = compress.encode("zstd", "hello", { level = 1 })
            local decompressed = compress.decode("zstd", compressed)
            expect(decompressed).toBe("hello")

            local compressed2 = compress.encode("zstd", "hello", { level = 22 })
            local decompressed2 = compress.decode("zstd", compressed2)
            expect(decompressed2).toBe("hello")
        end)

        test("brotli level range", function()
            local compressed = compress.encode("brotli", "hello", { level = 0 })
            local decompressed = compress.decode("brotli", compressed)
            expect(decompressed).toBe("hello")

            local compressed2 = compress.encode("brotli", "hello", { level = 11 })
            local decompressed2 = compress.decode("brotli", compressed2)
            expect(decompressed2).toBe("hello")
        end)

        test("level out of range returns (nil, err)", function()
            local val, err = compress.encode("gzip", "hello", { level = 99 })
            expect(val).toBeNil()
            expect(err).toContain("level out of range")
        end)

        test("non-integer level returns (nil, err)", function()
            local val, err = compress.encode("gzip", "hello", { level = 6.7 })
            expect(val).toBeNil()
            expect(err).toContain("level must be an integer")
        end)

        test("invalid level type returns (nil, err)", function()
            local val, err = compress.encode("gzip", "hello", { level = "fast" })
            expect(val).toBeNil()
            expect(err).toContain("invalid level type")
        end)

        test("unknown format returns (nil, err)", function()
            local val, err = compress.encode("lz4", "hello")
            expect(val).toBeNil()
            expect(err).toContain("unknown format 'lz4'")
        end)

        test("case-sensitive format", function()
            local val, err = compress.encode("GZIP", "hello")
            expect(val).toBeNil()
            expect(err).toContain("unknown format 'GZIP'")
        end)
    end)

    -- decode ------------------------------------------------------------------

    describe("decode", function()
        test("empty string returns (nil, err)", function()
            for _, fmt in ipairs({ "gzip", "zlib", "zstd", "brotli" }) do
                local val, err = compress.decode(fmt, "")
                expect(val).toBeNil()
                expect(err).toContain("compress.decode:")
            end
        end)

        test("corrupt data returns (nil, err)", function()
            for _, fmt in ipairs({ "gzip", "zlib", "zstd", "brotli" }) do
                local val, err = compress.decode(fmt, "not compressed data")
                expect(val).toBeNil()
                expect(err).toContain("compress.decode:")
            end
        end)

        test("cross-format incompatible", function()
            local compressed = compress.encode("gzip", "hello")
            local val, err = compress.decode("zlib", compressed)
            expect(val).toBeNil()
            expect(err).toContain("compress.decode:")
        end)

        test("unknown format returns (nil, err)", function()
            local val, err = compress.decode("lz4", "hello")
            expect(val).toBeNil()
            expect(err).toContain("unknown format 'lz4'")
        end)

        test("max_size exceeded returns (nil, err)", function()
            local big = string.rep("a", 1000)
            local compressed = compress.encode("gzip", big)
            local val, err = compress.decode("gzip", compressed, { max_size = 10 })
            expect(val).toBeNil()
            expect(err).toContain("output exceeds max_size")
        end)

        test("max_size invalid type returns (nil, err)", function()
            local val, err = compress.decode("gzip", "data", { max_size = "1MB" })
            expect(val).toBeNil()
            expect(err).toContain("invalid max_size type")
        end)

        test("max_size non-integer returns (nil, err)", function()
            local val, err = compress.decode("gzip", "data", { max_size = 1.5 })
            expect(val).toBeNil()
            expect(err).toContain("max_size must be an integer")
        end)

        test("max_size zero returns (nil, err)", function()
            local val, err = compress.decode("gzip", "data", { max_size = 0 })
            expect(val).toBeNil()
            expect(err).toContain("max_size must be > 0")
        end)

        test("max_size negative returns (nil, err)", function()
            local val, err = compress.decode("gzip", "data", { max_size = -1 })
            expect(val).toBeNil()
            expect(err).toContain("max_size must be > 0")
        end)

        test("explicit max_size allows large decode", function()
            local big = string.rep("x", 5000)
            local compressed = compress.encode("zlib", big)
            local decompressed, err = compress.decode("zlib", compressed, { max_size = 10000 })
            expect(err).toBeNil()
            expect(decompressed).toBe(big)
        end)
    end)

end)
