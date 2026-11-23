-- lib:crypto on neoc-vm and neoc-vm2: what both must answer the same.
local crypto = require("lib:crypto")
local hex = require("lib:hex")
local out = {}

local function unhex(text)
	return (hex.decode(text))
end

out.algorithms = crypto.algorithms()
local sizes = {}
for _, algo in crypto.algorithms() do
	sizes[algo] = { (crypto.key_size(algo)), (crypto.nonce_size(algo)), (crypto.tag_size(algo)) }
end
out.sizes = sizes

-- NIST GCM test case 4 (AES-128) and RFC 8439 section 2.8.2 (ChaCha20-Poly1305), with AAD
local pt = unhex("d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39")
local aad = unhex("feedfacedeadbeeffeedfacedeadbeefabaddad2")
local sealed = crypto.encrypt("aes_128_gcm", unhex("feffe9928665731c6d6a8f9467308308"), unhex("cafebabefacedbaddecaf888"), pt, aad)
out.aes_128_gcm = hex.encode(sealed)

local round = {}
for _, algo in crypto.algorithms() do
	local key = ("k"):rep((crypto.key_size(algo)))
	local nonce = ("n"):rep((crypto.nonce_size(algo)))
	local ct = crypto.encrypt(algo, key, nonce, "hello, world")
	local back, err = crypto.decrypt(algo, key, nonce, ct)
	round[algo] = { hex.encode(ct), back, err == nil }
end
out.round_trip = round

local key, nonce = ("k"):rep(32), ("n"):rep(12)
local ct1 = crypto.encrypt("aes_256_gcm", key, nonce, "msg")
local ct2 = crypto.encrypt("aes_256_gcm", key, nonce, "msg", "")
out.omitted_aad = { ct1 == ct2, (crypto.decrypt("aes_256_gcm", key, nonce, ct2)), (crypto.decrypt("aes_256_gcm", key, nonce, ct1, ""))}
local empty = crypto.encrypt("aes_256_gcm", key, nonce, "")
out.empty = { #empty, (crypto.decrypt("aes_256_gcm", key, nonce, empty)) }

-- failures are `(nil, err)`; what `err` is differs (see crypto.vm2.luau)
local sealed_a = crypto.encrypt("aes_256_gcm", key, nonce, "msg", "header-a")
for name, call in {
	aad_mismatch = function() return crypto.decrypt("aes_256_gcm", key, nonce, sealed_a, "header-b") end,
	wrong_key = function() return crypto.decrypt("aes_256_gcm", ("x"):rep(32), nonce, ct1) end,
	short_key = function() return crypto.encrypt("aes_256_gcm", "short", nonce, "msg") end,
	short_nonce = function() return crypto.encrypt("aes_256_gcm", key, "short", "msg") end,
	short_ciphertext = function() return crypto.decrypt("aes_256_gcm", key, nonce, "short") end,
} do
	local value, why = call()
	out["fails_" .. name] = { value == nil, why ~= nil }
end

return out
