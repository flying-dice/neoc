-- lib:pkey on neoc-vm and neoc-vm2: what both must answer the same. Generated keys are random, so they are
-- checked by what they do; Ed25519 signing is deterministic, so a fixed key (RFC 8032 test 1) is checked byte for byte.
local pkey = require("lib:pkey")
local hex = require("lib:hex")
local out = {}

out.algorithms = pkey.algorithms()

local seed = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"
local fixed = assert(pkey.KeyPair.from_der("ed25519", (hex.decode("302e020100300506032b657004220420" .. seed))))
out.rfc8032 = hex.encode((fixed:sign("")))
out.rfc8032_public = hex.encode((fixed:public_key():to_der()))

local per_algo = {}
for _, algo in pkey.algorithms() do
	local pair = assert(pkey.KeyPair.generate(algo))
	local public = pair:public_key()
	local sig = assert(pair:sign("attached message"))
	local pem = assert(pair:to_pem())
	local public_pem = assert(public:to_pem())
	per_algo[algo] = {
		verifies = public:verify("attached message", sig),
		tampered = public:verify("tampered", sig),
		garbage = public:verify("attached message", "not a real signature"),
		foreign = assert(pkey.KeyPair.generate(algo)):public_key():verify("attached message", sig),
		empty_message = public:verify("", (pair:sign(""))),
		binary_message = public:verify("\0\1\254\255", (pair:sign("\0\1\254\255"))),
		private_pem = pem:match("^%-%-%-%-%-BEGIN PRIVATE KEY%-%-%-%-%-") ~= nil,
		public_pem = public_pem:match("^%-%-%-%-%-BEGIN PUBLIC KEY%-%-%-%-%-") ~= nil,
		reloaded_pem = public:verify("msg", (assert(pkey.KeyPair.from_pem(algo, pem)):sign("msg"))),
		reloaded_der = public:verify("msg", (assert(pkey.KeyPair.from_der(algo, (pair:to_der()))):sign("msg"))),
		reloaded_public = assert(pkey.PublicKey.from_pem(algo, public_pem)):verify("attached message", sig),
		reloaded_public_der = assert(pkey.PublicKey.from_der(algo, (public:to_der()))):verify("attached message", sig),
		tostring = { tostring(pair), tostring(public) },
	}
end
out.per_algo = per_algo

out.signature_lengths = {
	ed25519 = #assert(pkey.KeyPair.generate("ed25519")):sign("msg"),
	ecdsa = #assert(pkey.KeyPair.generate("ecdsa-p256")):sign("msg"),
}

-- failures are `(nil, err)`; what `err` is differs (see pkey.vm2.luau)
local ed_pem = assert(assert(pkey.KeyPair.generate("ed25519")):to_pem())
for name, call in {
	wrong_algorithm = function() return pkey.KeyPair.from_pem("ecdsa-p256", ed_pem) end,
	malformed_pem = function() return pkey.KeyPair.from_pem("ed25519", "not pem") end,
	malformed_der = function() return pkey.KeyPair.from_der("ed25519", "\1\2\3") end,
	malformed_public_pem = function() return pkey.PublicKey.from_pem("ed25519", "not pem") end,
} do
	local value, why = call()
	out["fails_" .. name] = { value == nil, why ~= nil }
end

return out
