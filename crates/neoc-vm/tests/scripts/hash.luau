--!strict
-- lib:hash as a script sees it: its answers (hash.json), then what it asserts.
local hash = require("lib:hash")
local hex = require("lib:hex")
local out = {}

local algos = { "sha256", "sha384", "sha512", "sha3_256", "sha3_512", "blake3", "xxh3", "xxh64", "md5", "sha1" }
out.algorithms = hash.algorithms()

local one_shot, empty, streamed = {}, {}, {}
for _, algo in algos do
  one_shot[algo] = hex.encode(hash.hash(algo :: any, "abc"))
  empty[algo] = hex.encode(hash.hash(algo :: any, ""))
  local h = hash.Hasher.new(algo :: any)
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

-- what it asserts
do
  local h = hash.Hasher.new("sha3_512")
  assert(h.algorithm == "sha3_512")
  assert(tostring(h:update("a")) == "Hasher(sha3_512)")
  assert(h:update("b"):update("c"):digest() == hash.hash("sha3_512", "abc"))

  -- deliberate refusal tests: an algorithm that does not exist
  local ok, refused = pcall(hash.hash, "md4" :: any, "x")
  assert(not ok and tostring(refused):find("lib:hash.hash: argument #1 'algorithm'", 1, true))
  local ok2, refused2 = pcall(hash.Hasher.new, "md4" :: any)
  assert(not ok2 and tostring(refused2):find("argument #1 'algorithm'", 1, true))

  assert(#hash.algorithms() == 10 and hash.algorithms()[1] == "sha256")
end

return out
