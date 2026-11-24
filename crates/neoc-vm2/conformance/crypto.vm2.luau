--!strict
-- intentional: failures are typed errors (invalid_key_length, invalid_nonce_length, ciphertext_too_short, decryption_failed, encryption_failed); the algorithm is a declared union, so an unknown name is a bad argument and key_size/nonce_size/tag_size cannot fail.
local crypto = require("lib:crypto")
local key, nonce = ("k"):rep(32), ("n"):rep(12)

local _, short_key = crypto.encrypt("aes_256_gcm", "short", nonce, "msg")
assert(short_key ~= nil and short_key.kind == "invalid_key_length" and short_key.expected == 32 and short_key.got == 5)
assert(short_key.op == "lib:crypto.encrypt")
assert(tostring(short_key):find("^lib:crypto%.encrypt: invalid_key_length: "))

local _, short_nonce = crypto.encrypt("aes_256_gcm", key, "short", "msg")
assert(short_nonce ~= nil and short_nonce.kind == "invalid_nonce_length" and short_nonce.expected == 12 and short_nonce.got == 5)

local _, short = crypto.decrypt("aes_256_gcm", key, nonce, "short")
assert(short ~= nil and short.kind == "ciphertext_too_short" and short.minimum == 16 and short.got == 5)

local sealed = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", "a"))
local value, mismatch = crypto.decrypt("aes_256_gcm", key, nonce, sealed, "b")
assert(value == nil and mismatch ~= nil and mismatch.kind == "decryption_failed")
local _, wrong_key = crypto.decrypt("aes_256_gcm", ("x"):rep(32), nonce, sealed, "a")
assert(wrong_key ~= nil and wrong_key.kind == "decryption_failed" and wrong_key.message == mismatch.message, "one generic message, no oracle")

assert(crypto.key_size("aes_128_gcm") == 16 and crypto.nonce_size("chacha20_poly1305") == 12 and crypto.tag_size("aes_256_gcm") == 16)

-- deliberate refusal tests: an algorithm that does not exist
local ok, refused = pcall(crypto.key_size, "aes_512_gcm" :: any)
assert(not ok and tostring(refused):find("lib:crypto.key_size: argument #1 'algorithm'", 1, true))
local ok2, refused2 = pcall(function()
	crypto.encrypt("aes_512_gcm" :: any, key, nonce, "msg")
end)
assert(not ok2 and tostring(refused2):find("argument #1 'algorithm'", 1, true))
