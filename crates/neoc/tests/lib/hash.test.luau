local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local hash = require("lib:hash")

-- Convert raw bytes to lowercase hex for readable assertions.
local function to_hex(bytes: string): string
    local parts = {}
    for i = 1, #bytes do
        parts[i] = string.format("%02x", string.byte(bytes, i))
    end
    return table.concat(parts)
end

describe("hash", function()

    -- hash (one-shot) -------------------------------------------------------

    describe("hash", function()
        test("sha256 known vector", function()
            local result = hash.hash("sha256", "abc")
            expect(to_hex(result)).toBe(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            )
        end)

        test("sha384 known vector", function()
            local result = hash.hash("sha384", "abc")
            expect(to_hex(result)).toBe(
                "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed"
                .. "8086072ba1e7cc2358baeca134c825a7"
            )
        end)

        test("sha512 known vector", function()
            local result = hash.hash("sha512", "abc")
            expect(to_hex(result)).toBe(
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a"
                .. "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
            )
        end)

        test("sha3_256 known vector", function()
            local result = hash.hash("sha3_256", "abc")
            expect(to_hex(result)).toBe(
                "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
            )
        end)

        test("sha3_512 known vector", function()
            local result = hash.hash("sha3_512", "abc")
            expect(to_hex(result)).toBe(
                "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e"
                .. "10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0"
            )
        end)

        test("blake3 known vector", function()
            local result = hash.hash("blake3", "abc")
            expect(to_hex(result)).toBe(
                "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
            )
        end)

        test("xxh3 empty input", function()
            local result = hash.hash("xxh3", "")
            expect(to_hex(result)).toBe("2d06800538d394c2")
        end)

        test("xxh64 empty input", function()
            local result = hash.hash("xxh64", "")
            expect(to_hex(result)).toBe("ef46db3751d8e999")
        end)

        test("md5 RFC 1321 vector", function()
            local result = hash.hash("md5", "abc")
            expect(to_hex(result)).toBe("900150983cd24fb0d6963f7d28e17f72")
        end)

        test("sha1 FIPS 180-1 vector", function()
            local result = hash.hash("sha1", "abc")
            expect(to_hex(result)).toBe("a9993e364706816aba3e25717850c26c9cd0d89d")
        end)

        test("empty input produces non-empty digest", function()
            local result = hash.hash("sha256", "")
            expect(#result).toBe(32)
        end)

        test("binary input round-trips", function()
            local raw = "\0\1\2\254\255"
            local result = hash.hash("sha256", raw)
            expect(to_hex(result)).toBe(
                "103597c5abb6113da596c18e9d1da69364eafe00a2bfaa8b12e53c44bd6b0429"
            )
        end)

        test("unknown algorithm returns (nil, err)", function()
            local val, err = hash.hash("md4", "x")
            expect(val).toBeNil()
            expect(err).toContain("hash.hash:")
            expect(err).toContain("unknown algorithm 'md4'")
        end)
    end)

    -- Hasher (streaming) ----------------------------------------------------

    describe("Hasher", function()
        test("streaming matches one-shot", function()
            local h = hash.Hasher.new("sha256")
            h:update("hello ")
            h:update("world")
            local streamed = h:digest()
            local oneshot = hash.hash("sha256", "hello world")
            expect(to_hex(streamed)).toBe(to_hex(oneshot))
        end)

        test("digest is idempotent", function()
            local h = hash.Hasher.new("sha256")
            h:update("hello")
            local a = h:digest()
            local b = h:digest()
            expect(to_hex(a)).toBe(to_hex(b))
        end)

        test("update after digest extends state", function()
            local h = hash.Hasher.new("sha256")
            h:update("hello ")
            local _ = h:digest()
            h:update("world")
            expect(to_hex(h:digest())).toBe(to_hex(hash.hash("sha256", "hello world")))
        end)

        test("algorithm() returns the name", function()
            local h = hash.Hasher.new("blake3")
            expect(h:algorithm()).toBe("blake3")
        end)

        test("tostring includes algorithm name", function()
            local h = hash.Hasher.new("sha3_512")
            expect(tostring(h)).toBe("Hasher(sha3_512)")
        end)

        test("unknown algorithm returns (nil, err)", function()
            local val, err = hash.Hasher.new("md4")
            expect(val).toBeNil()
            expect(err).toContain("hash.Hasher.new:")
        end)

        test("empty input via streaming matches one-shot", function()
            local h = hash.Hasher.new("sha256")
            local streamed = h:digest()
            local oneshot = hash.hash("sha256", "")
            expect(to_hex(streamed)).toBe(to_hex(oneshot))
        end)

        test("large streamed input matches one-shot", function()
            local chunk = string.rep("x", 4096)
            local h = hash.Hasher.new("sha256")
            for _ = 1, 64 do h:update(chunk) end
            local total = string.rep("x", 4096 * 64)
            expect(to_hex(h:digest())).toBe(to_hex(hash.hash("sha256", total)))
        end)
    end)

    -- algorithms ------------------------------------------------------------

    describe("algorithms", function()
        test("returns canonical ordered list", function()
            local algos = hash.algorithms()
            expect(algos[1]).toBe("sha256")
            expect(algos[2]).toBe("sha384")
            expect(algos[3]).toBe("sha512")
            expect(algos[4]).toBe("sha3_256")
            expect(algos[5]).toBe("sha3_512")
            expect(algos[6]).toBe("blake3")
            expect(algos[7]).toBe("xxh3")
            expect(algos[8]).toBe("xxh64")
            expect(algos[9]).toBe("md5")
            expect(algos[10]).toBe("sha1")
            expect(#algos).toBe(10)
        end)

        test("every listed algorithm can construct a Hasher", function()
            for _, algo in ipairs(hash.algorithms()) do
                local h, err = hash.Hasher.new(algo)
                expect(err).toBeNil()
                expect(h).toBeTruthy()
            end
        end)
    end)

end)
