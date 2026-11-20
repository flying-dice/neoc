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

    -- byte stability ----------------------------------------------------------
    -- The encoders are pinned, not just round-tripped: a dependency upgrade
    -- (flate2 backend, zstd, brotli) that changes the bytes `encode` emits must
    -- be a deliberate, recorded change, so it fails here first. Expected values
    -- were captured from the pre-upgrade build (identical to the current one).

    describe("byte stability", function()
        local hex = require("lib:hex")
        local data = string.rep("neoc byte-stability probe 0123456789\n", 20)
        local pinned = {
            { "gzip", 1, "1f8b08000000000004ffedd2b90900211040d17cab980616bc8f721c994010159dc4eead4330ffd1e735ea197033fd8b13965a78c3981d098454da58e743fc5ef4165cace000a73b00a6e4020000" },
            { "gzip", 6, "1f8b08000000000000ffedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde800a73b00a6e4020000" },
            { "gzip", 9, "1f8b08000000000002ffedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde800a73b00a6e4020000" },
            { "zlib", 1, "7801edd2b90900211040d17cab980616bc8f721c994010159dc4eead4330ffd1e735ea197033fd8b13965a78c3981d098454da58e743fc5ef4165cace0003d25ee49" },
            { "zlib", 6, "789cedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde8003d25ee49" },
            { "zlib", 9, "78daedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde8003d25ee49" },
            { "zstd", 1, "28b52ffd004875010054026e656f6320627974652d73746162696c6974792070726f626520303132333435363738390a0100e5852af504" },
            { "zstd", 3, "28b52ffd005875010054026e656f6320627974652d73746162696c6974792070726f626520303132333435363738390a0100e5852af504" },
            { "zstd", 19, "28b52ffd006875010054026e656f6320627974652d73746162696c6974792070726f626520303132333435363738390a0100e5852af504" },
            { "brotli", 1, "8b71010080aaaaaaeaffae97931c2e0ab0ecb7a38a821c0440414541552e0a0770f12b4da4b8c106639e11359b64027e25fb74202fd60fe70a12381f62caa5b66ec6ef18" },
            { "brotli", 5, "1be30200c4dc467d255d28039fb7d5c9552762b10eaab02c0d9bbe60ff514b9107e7ba1121a65c6aeb630a04" },
            { "brotli", 11, "1be302f8c5e6469d9208850df46f1577559cb3580735583478838d58ecff411acf736e38582de5525b1f9300" },
        }
        for _, p in ipairs(pinned) do
            test(p[1] .. " level " .. p[2] .. " output is stable", function()
                local out, err = compress.encode(p[1], data, { level = p[2] })
                expect(err).toBeNil()
                expect(hex.encode(out)).toBe(p[3])
            end)
        end
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
