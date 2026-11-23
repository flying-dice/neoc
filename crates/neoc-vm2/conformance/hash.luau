-- lib:hash on neoc-vm and neoc-vm2: what both must answer the same.
local hash = require("lib:hash")
local hex = require("lib:hex")
local out = {}

local algos = { "sha256", "sha384", "sha512", "sha3_256", "sha3_512", "blake3", "xxh3", "xxh64", "md5", "sha1" }
out.algorithms = hash.algorithms()

local one_shot, empty, streamed = {}, {}, {}
for _, algo in algos do
	one_shot[algo] = hex.encode(hash.hash(algo, "abc"))
	empty[algo] = hex.encode(hash.hash(algo, ""))
	local h = hash.Hasher.new(algo)
	h:update("a")
	h:update("bc")
	streamed[algo] = hex.encode(h:digest())
end
out.one_shot = one_shot
out.empty = empty
out.streamed = streamed

local h = hash.Hasher.new("sha256")
h:update("hello ")
local first = h:digest()
out.idempotent = h:digest() == first
h:update("world")
out.continues = h:digest() == hash.hash("sha256", "hello world")
out.tostring = tostring(h)
out.binary = hex.encode(hash.hash("sha256", "\0\1\2\254\255"))
return out
