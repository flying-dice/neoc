-- intentional: `algorithm` is a Hasher field (was a method), `update` returns the hasher, and an unknown algorithm is refused as a bad argument (it was a returned error), because the algorithm is a declared union.
local hash = require("lib:hash")

local h = hash.Hasher.new("sha3_512")
assert(h.algorithm == "sha3_512")
assert(tostring(h:update("a")) == "Hasher(sha3_512)")
assert(h:update("b"):update("c"):digest() == hash.hash("sha3_512", "abc"))

local ok, refused = pcall(hash.hash, "md4", "x")
assert(not ok and tostring(refused):find("lib:hash.hash: argument #1 'algorithm'", 1, true))
local ok2, refused2 = pcall(hash.Hasher.new, "md4")
assert(not ok2 and tostring(refused2):find("argument #1 'algorithm'", 1, true))

assert(#hash.algorithms() == 10 and hash.algorithms()[1] == "sha256")
