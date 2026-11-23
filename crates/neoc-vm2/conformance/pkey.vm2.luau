-- intentional: failures are typed errors { kind = "invalid_key" | "generate_failed" | "sign_failed" | "encode_failed", reason }; `algorithm` is a field of KeyPair and PublicKey (was a method); the algorithm is a declared union, so an unknown name is a bad argument.
local pkey = require("lib:pkey")

local pair = assert(pkey.KeyPair.generate("ecdsa-p256"))
assert(pair.algorithm == "ecdsa-p256" and pair:public_key().algorithm == "ecdsa-p256")

local pem = assert(pair:to_pem())
local value, err = pkey.KeyPair.from_pem("ed25519", pem)
assert(value == nil and err.kind == "invalid_key" and type(err.reason) == "string")
assert(err.op == "lib:pkey.KeyPair.from_pem")
assert(tostring(err):find("^lib:pkey%.KeyPair%.from_pem: invalid_key: "))

local _, der = pkey.PublicKey.from_der("rsa-pss-sha256", "\1\2\3")
assert(der.kind == "invalid_key" and der.op == "lib:pkey.PublicKey.from_der")

local ok, refused = pcall(pkey.KeyPair.generate, "rsa-1024")
assert(not ok and tostring(refused):find("lib:pkey.KeyPair.generate: argument #1 'algorithm'", 1, true))
