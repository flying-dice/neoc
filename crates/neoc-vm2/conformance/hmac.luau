-- lib:hmac on neoc-vm and neoc-vm2: what both must answer the same.
local hmac = require("lib:hmac")
local hex = require("lib:hex")
local out = {}

out.algorithms = hmac.algorithms()

local rfc, streamed = {}, {}
for _, algo in hmac.algorithms() do
	rfc[algo] = hex.encode(hmac.hmac(algo, "Jefe", "what do ya want for nothing?"))
	local m = hmac.Hmac.new(algo, "Jefe")
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
return out
