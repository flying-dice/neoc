local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local pkey = require("lib:pkey")

local ALGOS = {
    "ed25519",
    "ecdsa-p256",
    "rsa-pss-sha256",
    "rsa-pkcs1v15-sha256",
}

describe("pkey", function()

    -- KeyPair.generate ------------------------------------------------------

    describe("KeyPair.generate", function()
        test("produces a usable keypair for every algorithm", function()
            for _, algo in ipairs(ALGOS) do
                local kp, err = pkey.KeyPair.generate(algo)
                expect(err).toBeNil()
                expect(kp).toBeTruthy()
                expect(kp:algorithm()).toBe(algo)
            end
        end)

        test("unknown algorithm returns (nil, err)", function()
            local kp, err = pkey.KeyPair.generate("rsa-1024")
            expect(kp).toBeNil()
            expect(err).toContain("KeyPair.generate:")
            expect(err).toContain("unknown algorithm 'rsa-1024'")
        end)
    end)

    -- sign / verify ---------------------------------------------------------

    describe("sign and verify", function()
        test("a fresh signature verifies under the public key", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                local sig = assert(kp:sign("attached message"))
                expect(kp:public_key():verify("attached message", sig)).toBe(true)
            end
        end)

        test("a tampered message does not verify", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                local sig = assert(kp:sign("original"))
                expect(kp:public_key():verify("tampered", sig)).toBe(false)
            end
        end)

        test("a foreign key's signature does not verify", function()
            for _, algo in ipairs(ALGOS) do
                local a = assert(pkey.KeyPair.generate(algo))
                local b = assert(pkey.KeyPair.generate(algo))
                local sig = assert(a:sign("msg"))
                expect(b:public_key():verify("msg", sig)).toBe(false)
            end
        end)

        test("a malformed signature returns false, not an error", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                expect(kp:public_key():verify("msg", "garbage")).toBe(false)
            end
        end)

        test("the empty message signs and verifies", function()
            local kp = assert(pkey.KeyPair.generate("ed25519"))
            local sig = assert(kp:sign(""))
            expect(kp:public_key():verify("", sig)).toBe(true)
        end)

        test("a binary message round-trips", function()
            local msg = "\0\1\2\254\255\128"
            local kp = assert(pkey.KeyPair.generate("ed25519"))
            local sig = assert(kp:sign(msg))
            expect(kp:public_key():verify(msg, sig)).toBe(true)
        end)

        test("Ed25519 signatures are 64 bytes", function()
            local kp = assert(pkey.KeyPair.generate("ed25519"))
            expect(#kp:sign("msg")).toBe(64)
        end)

        test("ECDSA P-256 signatures are the fixed 64-byte form", function()
            local kp = assert(pkey.KeyPair.generate("ecdsa-p256"))
            expect(#kp:sign("msg")).toBe(64)
        end)
    end)

    -- cross-algorithm and scheme confusion ----------------------------------

    describe("cross-algorithm and scheme confusion", function()
        test("a signature does not verify under a different-algorithm key", function()
            -- An Ed25519 sig is 64 bytes — the same length as ECDSA P-256 r‖s,
            -- but not a valid point pair. verify must fold to false, never raise.
            local ed = assert(pkey.KeyPair.generate("ed25519"))
            local ec = assert(pkey.KeyPair.generate("ecdsa-p256"))
            local sig = assert(ed:sign("msg"))
            expect(ec:public_key():verify("msg", sig)).toBe(false)
        end)

        test("an ASN.1 DER ECDSA signature is rejected by the fixed-form verifier", function()
            -- Node/Go emit ECDSA sigs as DER SEQUENCE{r,s} by default; this lib
            -- expects the fixed 64-byte r‖s (JOSE ES256). Re-encode a real sig as
            -- minimal DER and confirm it is rejected — pins the interop contract.
            local kp = assert(pkey.KeyPair.generate("ecdsa-p256"))
            local fixed = assert(kp:sign("msg"))
            expect(#fixed).toBe(64)
            local function der_int(b)
                local i = 1
                while i < #b and b:byte(i) == 0 do i = i + 1 end
                b = b:sub(i)
                if b:byte(1) >= 0x80 then b = "\0" .. b end
                return string.char(0x02, #b) .. b
            end
            local body = der_int(fixed:sub(1, 32)) .. der_int(fixed:sub(33, 64))
            local der = string.char(0x30, #body) .. body
            expect(kp:public_key():verify("msg", der)).toBe(false)
        end)

        test("a PSS signature does not verify under the key loaded as PKCS#1 v1.5", function()
            -- The RSA scheme is the caller's declared intent, not recorded in the
            -- key. Sign PSS, reload the public key as PKCS#1 v1.5, verify → false.
            local kp = assert(pkey.KeyPair.generate("rsa-pss-sha256"))
            local sig = assert(kp:sign("msg"))
            local pem = assert(kp:public_key():to_pem())
            local pub = assert(pkey.PublicKey.from_pem("rsa-pkcs1v15-sha256", pem))
            expect(pub:verify("msg", sig)).toBe(false)
        end)
    end)

    -- serialisation ---------------------------------------------------------

    describe("serialisation", function()
        test("private PKCS#8 PEM round-trips and still signs", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                local pem = assert(kp:to_pem())
                expect(pem).toContain("BEGIN PRIVATE KEY")
                local loaded = assert(pkey.KeyPair.from_pem(algo, pem))
                local sig = assert(loaded:sign("msg"))
                expect(kp:public_key():verify("msg", sig)).toBe(true)
            end
        end)

        test("private PKCS#8 DER round-trips and still signs", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                local der = assert(kp:to_der())
                local loaded = assert(pkey.KeyPair.from_der(algo, der))
                local sig = assert(loaded:sign("msg"))
                expect(kp:public_key():verify("msg", sig)).toBe(true)
            end
        end)

        test("public SPKI PEM round-trips and verifies", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                local sig = assert(kp:sign("msg"))
                local pem = assert(kp:public_key():to_pem())
                expect(pem).toContain("BEGIN PUBLIC KEY")
                local pub = assert(pkey.PublicKey.from_pem(algo, pem))
                expect(pub:verify("msg", sig)).toBe(true)
            end
        end)

        test("public SPKI DER round-trips and verifies", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                local sig = assert(kp:sign("msg"))
                local der = assert(kp:public_key():to_der())
                local pub = assert(pkey.PublicKey.from_der(algo, der))
                expect(pub:verify("msg", sig)).toBe(true)
            end
        end)

        test("loading a key under the wrong algorithm returns (nil, err)", function()
            local kp = assert(pkey.KeyPair.generate("ed25519"))
            local pem = assert(kp:to_pem())
            local loaded, err = pkey.KeyPair.from_pem("ecdsa-p256", pem)
            expect(loaded).toBeNil()
            expect(err).toContain("KeyPair.from_pem:")
        end)

        test("malformed PEM returns (nil, err)", function()
            local loaded, err = pkey.KeyPair.from_pem("ed25519", "not a pem")
            expect(loaded).toBeNil()
            expect(err).toContain("KeyPair.from_pem:")
        end)

        test("malformed DER returns (nil, err)", function()
            local loaded, err = pkey.KeyPair.from_der("ed25519", "not der bytes")
            expect(loaded).toBeNil()
            expect(err).toContain("KeyPair.from_der:")
        end)
    end)

    -- public_key + metadata -------------------------------------------------

    describe("public_key and metadata", function()
        test("the extracted public key reports the same algorithm", function()
            for _, algo in ipairs(ALGOS) do
                local kp = assert(pkey.KeyPair.generate(algo))
                expect(kp:public_key():algorithm()).toBe(algo)
            end
        end)

        test("tostring names the type and algorithm", function()
            local kp = assert(pkey.KeyPair.generate("ed25519"))
            expect(tostring(kp)).toBe("KeyPair(ed25519)")
            expect(tostring(kp:public_key())).toBe("PublicKey(ed25519)")
        end)
    end)

    -- algorithms ------------------------------------------------------------

    describe("algorithms", function()
        test("returns the canonical ordered list", function()
            local algos = pkey.algorithms()
            expect(algos[1]).toBe("ed25519")
            expect(algos[2]).toBe("ecdsa-p256")
            expect(algos[3]).toBe("rsa-pss-sha256")
            expect(algos[4]).toBe("rsa-pkcs1v15-sha256")
            expect(#algos).toBe(4)
        end)

        test("every listed algorithm can generate a keypair", function()
            for _, algo in ipairs(pkey.algorithms()) do
                local kp, err = pkey.KeyPair.generate(algo)
                expect(err).toBeNil()
                expect(kp).toBeTruthy()
            end
        end)
    end)

end)
