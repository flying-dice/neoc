local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local hmac = require("lib:hmac")

-- Convert raw bytes to lowercase hex for readable assertions.
local function to_hex(bytes: string): string
    local parts = {}
    for i = 1, #bytes do
        parts[i] = string.format("%02x", string.byte(bytes, i))
    end
    return table.concat(parts)
end

describe("hmac", function()

    -- hmac (one-shot) -------------------------------------------------------

    describe("hmac", function()
        -- RFC 4231 Test Case 2: key "Jefe", data "what do ya want for nothing?".
        local KEY = "Jefe"
        local DATA = "what do ya want for nothing?"

        test("sha256 RFC 4231 vector", function()
            expect(to_hex(hmac.hmac("sha256", KEY, DATA))).toBe(
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
            )
        end)

        test("sha384 RFC 4231 vector", function()
            expect(to_hex(hmac.hmac("sha384", KEY, DATA))).toBe(
                "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e"
                .. "8e2240ca5e69e2c78b3239ecfab21649"
            )
        end)

        test("sha512 RFC 4231 vector", function()
            expect(to_hex(hmac.hmac("sha512", KEY, DATA))).toBe(
                "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea250554"
                .. "9758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737"
            )
        end)

        test("different keys produce different tags", function()
            local a = hmac.hmac("sha256", "key-a", "msg")
            local b = hmac.hmac("sha256", "key-b", "msg")
            expect(to_hex(a) == to_hex(b)).toBe(false)
        end)

        test("empty key and message produce a full-width tag", function()
            expect(#hmac.hmac("sha256", "", "")).toBe(32)
        end)

        test("unknown algorithm returns (nil, err)", function()
            local val, err = hmac.hmac("sha1", "k", "x")
            expect(val).toBeNil()
            expect(err).toContain("hmac.hmac:")
            expect(err).toContain("unknown algorithm 'sha1'")
        end)
    end)

    -- Hmac (streaming) ------------------------------------------------------

    describe("Hmac", function()
        test("streaming matches one-shot", function()
            local m = hmac.Hmac.new("sha256", "secret")
            m:update("hello ")
            m:update("world")
            local streamed = m:digest()
            local oneshot = hmac.hmac("sha256", "secret", "hello world")
            expect(to_hex(streamed)).toBe(to_hex(oneshot))
        end)

        test("digest is idempotent", function()
            local m = hmac.Hmac.new("sha256", "k")
            m:update("hello")
            expect(to_hex(m:digest())).toBe(to_hex(m:digest()))
        end)

        test("update after digest extends state", function()
            local m = hmac.Hmac.new("sha256", "k")
            m:update("hello ")
            local _ = m:digest()
            m:update("world")
            expect(to_hex(m:digest())).toBe(to_hex(hmac.hmac("sha256", "k", "hello world")))
        end)

        test("binary key and data round-trip", function()
            local key = "\0\1\2\254\255"
            local data = "\255\0\128"
            local m = hmac.Hmac.new("sha512", key)
            m:update(data)
            expect(to_hex(m:digest())).toBe(to_hex(hmac.hmac("sha512", key, data)))
        end)

        test("algorithm() returns the name", function()
            expect(hmac.Hmac.new("sha384", "k"):algorithm()).toBe("sha384")
        end)

        test("tostring includes algorithm name", function()
            expect(tostring(hmac.Hmac.new("sha512", "k"))).toBe("Hmac(sha512)")
        end)

        test("unknown algorithm returns (nil, err)", function()
            local val, err = hmac.Hmac.new("md5", "k")
            expect(val).toBeNil()
            expect(err).toContain("hmac.Hmac.new:")
        end)

        test("large streamed input matches one-shot", function()
            local chunk = string.rep("x", 4096)
            local m = hmac.Hmac.new("sha256", "k")
            for _ = 1, 64 do m:update(chunk) end
            local total = string.rep("x", 4096 * 64)
            expect(to_hex(m:digest())).toBe(to_hex(hmac.hmac("sha256", "k", total)))
        end)
    end)

    -- verify (constant-time) ------------------------------------------------

    describe("verify", function()
        test("accepts matching tags", function()
            local a = hmac.hmac("sha256", "k", "msg")
            local b = hmac.hmac("sha256", "k", "msg")
            expect(hmac.verify(a, b)).toBe(true)
        end)

        test("rejects a tampered tag", function()
            local a = hmac.hmac("sha256", "k", "msg")
            local b = hmac.hmac("sha256", "k", "tampered")
            expect(hmac.verify(a, b)).toBe(false)
        end)

        test("rejects a length mismatch", function()
            expect(hmac.verify("abc", "abcd")).toBe(false)
        end)

        test("empty tags match", function()
            expect(hmac.verify("", "")).toBe(true)
        end)
    end)

    -- algorithms ------------------------------------------------------------

    describe("algorithms", function()
        test("returns canonical ordered list", function()
            local algos = hmac.algorithms()
            expect(algos[1]).toBe("sha256")
            expect(algos[2]).toBe("sha384")
            expect(algos[3]).toBe("sha512")
            expect(#algos).toBe(3)
        end)

        test("every listed algorithm can construct an Hmac", function()
            for _, algo in ipairs(hmac.algorithms()) do
                local m, err = hmac.Hmac.new(algo, "k")
                expect(err).toBeNil()
                expect(m).toBeTruthy()
            end
        end)
    end)

end)
