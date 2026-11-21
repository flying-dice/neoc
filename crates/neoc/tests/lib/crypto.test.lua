local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local crypto = require("lib:crypto")
local hex    = require("lib:hex")

-- Convert raw bytes to lowercase hex for readable assertions.
local function to_hex(bytes)
    local parts = {}
    for i = 1, #bytes do
        parts[i] = string.format("%02x", string.byte(bytes, i))
    end
    return table.concat(parts)
end

describe("crypto", function()

    -- encrypt + decrypt round-trip ----------------------------------------

    describe("round-trip", function()
        for _, algo in ipairs(crypto.algorithms()) do
            test(("%s: encrypt → decrypt recovers plaintext"):format(algo), function()
                local key = string.rep("k", crypto.key_size(algo))
                local nonce = string.rep("n", crypto.nonce_size(algo))
                local pt = "hello, world"
                local ct, e1 = crypto.encrypt(algo, key, nonce, pt)
                expect(e1).toBeNil()
                expect(ct).toBeTruthy()
                local out, e2 = crypto.decrypt(algo, key, nonce, ct)
                expect(e2).toBeNil()
                expect(out).toBe(pt)
            end)

            test(("%s: ciphertext length is plaintext + 16-byte tag"):format(algo), function()
                local key = string.rep("k", crypto.key_size(algo))
                local nonce = string.rep("n", crypto.nonce_size(algo))
                local ct = assert(crypto.encrypt(algo, key, nonce, "msg"))
                expect(#ct).toBe(3 + 16)
            end)
        end
    end)

    -- known-answer vectors -------------------------------------------------

    describe("known vectors", function()
        test("aes_128_gcm NIST Case 4", function()
            local key   = assert(hex.decode("feffe9928665731c6d6a8f9467308308"))
            local nonce = assert(hex.decode("cafebabefacedbaddecaf888"))
            local pt    = assert(hex.decode(
                "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72"
                .. "1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39"
            ))
            local aad   = assert(hex.decode("feedfacedeadbeeffeedfacedeadbeefabaddad2"))
            local ct    = assert(crypto.encrypt("aes_128_gcm", key, nonce, pt, aad))
            expect(to_hex(ct)).toBe(
                "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e"
                .. "21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e0915bc94fbc"
                .. "3221a5db94fae95ae7121a47"
            )
        end)

        test("aes_256_gcm NIST Case 4", function()
            local key   = assert(hex.decode(
                "feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308"
            ))
            local nonce = assert(hex.decode("cafebabefacedbaddecaf888"))
            local pt    = assert(hex.decode(
                "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72"
                .. "1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39"
            ))
            local aad   = assert(hex.decode("feedfacedeadbeeffeedfacedeadbeefabaddad2"))
            local ct    = assert(crypto.encrypt("aes_256_gcm", key, nonce, pt, aad))
            expect(to_hex(ct)).toBe(
                "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa"
                .. "8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f66276fc6ece"
                .. "0f4e1768cddf8853bb2d551b"
            )
        end)

        test("chacha20_poly1305 RFC 8439 §2.8.2", function()
            local key   = assert(hex.decode(
                "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f"
            ))
            local nonce = assert(hex.decode("070000004041424344454647"))
            local pt    = "Ladies and Gentlemen of the class of '99: "
                .. "If I could offer you only one tip for the future, "
                .. "sunscreen would be it."
            local aad   = assert(hex.decode("50515253c0c1c2c3c4c5c6c7"))
            local ct    = assert(crypto.encrypt("chacha20_poly1305", key, nonce, pt, aad))
            expect(to_hex(ct)).toBe(
                "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6"
                .. "3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36"
                .. "92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc"
                .. "3ff4def08e4b7a9de576d26586cec64b6116"
                .. "1ae10b594f09e26a7e902ecbd0600691"
            )
        end)
    end)

    -- AAD binding ----------------------------------------------------------

    describe("aad", function()
        test("omitted AAD is identical to empty-string AAD", function()
            local key = string.rep("k", 32)
            local nonce = string.rep("n", 12)
            local ct1 = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg"))
            local ct2 = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", ""))
            expect(to_hex(ct1)).toBe(to_hex(ct2))
        end)

        test("AAD mismatch fails decrypt", function()
            local key = string.rep("k", 32)
            local nonce = string.rep("n", 12)
            local ct = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", "header-a"))
            local pt, e = crypto.decrypt("aes_256_gcm", key, nonce, ct, "header-b")
            expect(pt).toBeNil()
            expect(e).toContain("crypto.decrypt: decryption failed")
        end)

        test("missing AAD on decrypt fails when present on encrypt", function()
            local key = string.rep("k", 32)
            local nonce = string.rep("n", 12)
            local ct = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", "header"))
            local pt, e = crypto.decrypt("aes_256_gcm", key, nonce, ct)
            expect(pt).toBeNil()
            expect(e).toContain("crypto.decrypt: decryption failed")
        end)
    end)

    -- tampering ------------------------------------------------------------

    describe("integrity", function()
        test("tampered ciphertext fails decrypt", function()
            local key = string.rep("k", 32)
            local nonce = string.rep("n", 12)
            local ct = assert(crypto.encrypt("chacha20_poly1305", key, nonce, "msg"))
            local tampered = string.char(bit32.bxor(string.byte(ct, 1), 1))
                .. string.sub(ct, 2)
            local pt, e = crypto.decrypt("chacha20_poly1305", key, nonce, tampered)
            expect(pt).toBeNil()
            expect(e).toContain("crypto.decrypt: decryption failed")
        end)

        test("wrong key fails decrypt", function()
            local nonce = string.rep("n", 12)
            local ct = assert(crypto.encrypt("aes_256_gcm", string.rep("a", 32), nonce, "msg"))
            local pt, e = crypto.decrypt("aes_256_gcm", string.rep("b", 32), nonce, ct)
            expect(pt).toBeNil()
            expect(e).toContain("crypto.decrypt: decryption failed")
        end)

        test("wrong nonce fails decrypt", function()
            local key = string.rep("k", 32)
            local ct = assert(crypto.encrypt("aes_256_gcm", key, string.rep("a", 12), "msg"))
            local pt, e = crypto.decrypt("aes_256_gcm", key, string.rep("b", 12), ct)
            expect(pt).toBeNil()
            expect(e).toContain("crypto.decrypt: decryption failed")
        end)
    end)

    -- edge cases -----------------------------------------------------------

    describe("edge cases", function()
        test("empty plaintext encrypts to a tag-only ciphertext", function()
            local key = string.rep("k", 32)
            local nonce = string.rep("n", 12)
            local ct = assert(crypto.encrypt("aes_256_gcm", key, nonce, ""))
            expect(#ct).toBe(16)
            expect(assert(crypto.decrypt("aes_256_gcm", key, nonce, ct))).toBe("")
        end)

        test("binary plaintext round-trips", function()
            local key = string.rep("k", 32)
            local nonce = string.rep("n", 12)
            local pt = "\0\1\2\254\255"
            local ct = assert(crypto.encrypt("aes_256_gcm", key, nonce, pt))
            expect(to_hex(assert(crypto.decrypt("aes_256_gcm", key, nonce, ct))))
                .toBe(to_hex(pt))
        end)

        test("truncated ciphertext is rejected before backend", function()
            local key = string.rep("k", 32)
            local nonce = string.rep("n", 12)
            local pt, e = crypto.decrypt("aes_256_gcm", key, nonce, "short")
            expect(pt).toBeNil()
            expect(e).toContain("ciphertext shorter than")
        end)
    end)

    -- validation -----------------------------------------------------------

    describe("validation", function()
        test("wrong key length errors with named cause", function()
            local v, e = crypto.encrypt("aes_256_gcm", "short", string.rep("n", 12), "msg")
            expect(v).toBeNil()
            expect(e).toContain("crypto.encrypt:")
            expect(e).toContain("expected 32-byte key")
        end)

        test("wrong nonce length errors with named cause", function()
            local v, e = crypto.encrypt("aes_256_gcm", string.rep("k", 32), "short", "msg")
            expect(v).toBeNil()
            expect(e).toContain("crypto.encrypt:")
            expect(e).toContain("expected 12-byte nonce")
        end)

        test("unknown algorithm errors on encrypt", function()
            local v, e = crypto.encrypt("aes_512_gcm", "", "", "")
            expect(v).toBeNil()
            expect(e).toContain("unknown algorithm 'aes_512_gcm'")
        end)
    end)

    -- size queries ---------------------------------------------------------

    describe("sizes", function()
        test("key_size matches canonical AEAD lengths", function()
            expect(crypto.key_size("aes_128_gcm")).toBe(16)
            expect(crypto.key_size("aes_256_gcm")).toBe(32)
            expect(crypto.key_size("chacha20_poly1305")).toBe(32)
        end)

        test("nonce_size is 12 bytes for every supported algorithm", function()
            for _, algo in ipairs(crypto.algorithms()) do
                expect(crypto.nonce_size(algo)).toBe(12)
            end
        end)

        test("tag_size is 16 bytes for every supported algorithm", function()
            for _, algo in ipairs(crypto.algorithms()) do
                expect(crypto.tag_size(algo)).toBe(16)
            end
        end)

        test("unknown algorithm errors on size queries", function()
            local v, e = crypto.key_size("aes_512_gcm")
            expect(v).toBeNil()
            expect(e).toContain("crypto.key_size:")
            expect(e).toContain("unknown algorithm 'aes_512_gcm'")
        end)
    end)

    -- algorithms list ------------------------------------------------------

    describe("algorithms", function()
        test("returns canonical ordered list", function()
            local algos = crypto.algorithms()
            expect(algos[1]).toBe("aes_128_gcm")
            expect(algos[2]).toBe("aes_256_gcm")
            expect(algos[3]).toBe("chacha20_poly1305")
            expect(#algos).toBe(3)
        end)
    end)

end)
