--!strict
-- lib:crypto as a script sees it: its answers (crypto.json), then what it asserts.
local crypto = require("lib:crypto")
local hex = require("lib:hex")
local out = {}

local function unhex(text: string): string
  return (assert(hex.decode(text)))
end

out.algorithms = crypto.algorithms()
local sizes = {}
for _, algo in crypto.algorithms() do
  sizes[algo] = { (crypto.key_size(algo :: any)), (crypto.nonce_size(algo :: any)), (crypto.tag_size(algo :: any)) }
end
out.sizes = sizes

-- NIST GCM test case 4 (AES-128) and RFC 8439 section 2.8.2 (ChaCha20-Poly1305), with AAD
local pt = unhex(
  "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39"
)
local aad = unhex("feedfacedeadbeeffeedfacedeadbeefabaddad2")
local sealed = assert(
  crypto.encrypt("aes_128_gcm", unhex("feffe9928665731c6d6a8f9467308308"), unhex("cafebabefacedbaddecaf888"), pt, aad)
)
out.aes_128_gcm = hex.encode(sealed)

local round = {}
for _, algo in crypto.algorithms() do
  local key = ("k"):rep((crypto.key_size(algo :: any)))
  local nonce = ("n"):rep((crypto.nonce_size(algo :: any)))
  local ct = assert(crypto.encrypt(algo :: any, key, nonce, "hello, world"))
  local back, err = crypto.decrypt(algo :: any, key, nonce, ct)
  round[algo] = { hex.encode(ct), back, err == nil } :: { any }
end
out.round_trip = round

local key, nonce = ("k"):rep(32), ("n"):rep(12)
local ct1 = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg"))
local ct2 = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", ""))
out.omitted_aad = {
  ct1 == ct2,
  (crypto.decrypt("aes_256_gcm", key, nonce, ct2)),
  (crypto.decrypt("aes_256_gcm", key, nonce, ct1, "")),
} :: { any }
local empty = assert(crypto.encrypt("aes_256_gcm", key, nonce, ""))
out.empty = { #empty, (crypto.decrypt("aes_256_gcm", key, nonce, empty)) } :: { any }

-- failures are `(nil, err)`
local sealed_a = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", "header-a"))
for name, call in
  {
    aad_mismatch = function()
      return crypto.decrypt("aes_256_gcm", key, nonce, sealed_a, "header-b")
    end,
    wrong_key = function()
      return crypto.decrypt("aes_256_gcm", ("x"):rep(32), nonce, ct1)
    end,
    short_key = function()
      return crypto.encrypt("aes_256_gcm", "short", nonce, "msg")
    end,
    short_nonce = function()
      return crypto.encrypt("aes_256_gcm", key, "short", "msg")
    end,
    short_ciphertext = function()
      return crypto.decrypt("aes_256_gcm", key, nonce, "short")
    end,
  } :: { [string]: () -> (unknown, unknown) }
do
  local value, why = call()
  out["fails_" .. name] = { value == nil, why ~= nil }
end

-- what it asserts
do
  local key, nonce = ("k"):rep(32), ("n"):rep(12)

  local _, short_key = crypto.encrypt("aes_256_gcm", "short", nonce, "msg")
  assert(
    short_key ~= nil and short_key.kind == "invalid_key_length" and short_key.expected == 32 and short_key.got == 5
  )
  assert(short_key.op == "lib:crypto.encrypt")
  assert(tostring(short_key):find("^lib:crypto%.encrypt: invalid_key_length: "))

  local _, short_nonce = crypto.encrypt("aes_256_gcm", key, "short", "msg")
  assert(
    short_nonce ~= nil
      and short_nonce.kind == "invalid_nonce_length"
      and short_nonce.expected == 12
      and short_nonce.got == 5
  )

  local _, short = crypto.decrypt("aes_256_gcm", key, nonce, "short")
  assert(short ~= nil and short.kind == "ciphertext_too_short" and short.minimum == 16 and short.got == 5)

  local sealed = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", "a"))
  local value, mismatch = crypto.decrypt("aes_256_gcm", key, nonce, sealed, "b")
  assert(value == nil and mismatch ~= nil and mismatch.kind == "decryption_failed")
  local _, wrong_key = crypto.decrypt("aes_256_gcm", ("x"):rep(32), nonce, sealed, "a")
  assert(
    wrong_key ~= nil and wrong_key.kind == "decryption_failed" and wrong_key.message == mismatch.message,
    "one generic message, no oracle"
  )

  assert(
    crypto.key_size("aes_128_gcm") == 16
      and crypto.nonce_size("chacha20_poly1305") == 12
      and crypto.tag_size("aes_256_gcm") == 16
  )

  -- deliberate refusal tests: an algorithm that does not exist
  local ok, refused = pcall(crypto.key_size, "aes_512_gcm" :: any)
  assert(not ok and tostring(refused):find("lib:crypto.key_size: argument #1 'algorithm'", 1, true))
  local ok2, refused2 = pcall(function()
    crypto.encrypt("aes_512_gcm" :: any, key, nonce, "msg")
  end)
  assert(not ok2 and tostring(refused2):find("argument #1 'algorithm'", 1, true))
end

return out
