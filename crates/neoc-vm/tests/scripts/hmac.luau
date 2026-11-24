--!strict
-- lib:hmac as a script sees it: its answers (hmac.json), then what it asserts.
local hmac = require("lib:hmac")
local hex = require("lib:hex")
local out = {}

out.algorithms = hmac.algorithms()

local rfc, streamed = {}, {}
for _, algo in hmac.algorithms() do
  rfc[algo] = hex.encode(hmac.hmac(algo :: any, "Jefe", "what do ya want for nothing?"))
  local m = hmac.Hmac.new(algo :: any, "Jefe")
  m:update("what do ya want ")
  m:update("for nothing?")
  streamed[algo] = hex.encode(m:digest())
end
out.rfc4231 = rfc
out.streamed = streamed

local m = hmac.Hmac.new("sha256", "k")
m:update("hello ")
local first = m:digest()
out.idempotent = m:digest() == first
m:update("world")
out.continues = m:digest() == hmac.hmac("sha256", "k", "hello world")
out.tostring = tostring(m)

out.empty = #hmac.hmac("sha256", "", "")
out.binary = hex.encode(hmac.hmac("sha512", "\0\1\2\254\255", "\255\0\128"))

local tag = hmac.hmac("sha256", "k", "msg")
out.verify = {
  hmac.verify(tag, hmac.hmac("sha256", "k", "msg")),
  hmac.verify(tag, hmac.hmac("sha256", "k", "tampered")),
  hmac.verify("abc", "abcd"),
  hmac.verify("", ""),
}

-- what it asserts
do
  local m = hmac.Hmac.new("sha384", "k")
  assert(m.algorithm == "sha384")
  assert(tostring(m:update("a")) == "Hmac(sha384)")
  assert(m:update("b"):update("c"):digest() == hmac.hmac("sha384", "k", "abc"))

  -- deliberate refusal tests: an algorithm that is not supported
  local ok, refused = pcall(hmac.hmac, "sha1" :: any, "k", "x")
  assert(not ok and tostring(refused):find("lib:hmac.hmac: argument #1 'algorithm'", 1, true))
  local ok2, refused2 = pcall(hmac.Hmac.new, "md5" :: any, "k")
  assert(not ok2 and tostring(refused2):find("argument #1 'algorithm'", 1, true))
end

return out
