--!strict
-- intentional: `algorithm` is a Hmac field (was a method), `update` returns the MAC, and an unsupported algorithm is refused as a bad argument (it was a returned error), because the algorithm is a declared union.
local hmac = require("lib:hmac")

local m = hmac.Hmac.new("sha384", "k")
assert(m.algorithm == "sha384")
assert(tostring(m:update("a")) == "Hmac(sha384)")
assert(m:update("b"):update("c"):digest() == hmac.hmac("sha384", "k", "abc"))

-- deliberate refusal tests: an algorithm that is not supported
local ok, refused = pcall(hmac.hmac, "sha1" :: any, "k", "x")
assert(not ok and tostring(refused):find("lib:hmac.hmac: argument #1 'algorithm'", 1, true))
local ok2, refused2 = pcall(hmac.Hmac.new, "md5" :: any, "k")
assert(not ok2 and tostring(refused2):find("argument #1 'algorithm'", 1, true))
