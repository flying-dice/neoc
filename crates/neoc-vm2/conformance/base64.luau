-- lib:base64 on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
local b64 = require("lib:base64")
local out = {}

out.encode = b64.encode("hello")
out.encode_empty = b64.encode("")
out.padding = { b64.encode("hi"), b64.encode_no_pad("hi") }
out.url_safe = { b64.encode("\251\255\254"), b64.encode_url_safe("\251\255\254"), b64.encode_url_safe_no_pad("\251\255\254") }

local raw = "\0\1\2\254\255"
local decoded, err = b64.decode(b64.encode(raw))
out.round_trip = { decoded == raw, err == nil, #decoded }

local url, url_err = b64.decode_url_safe(b64.encode_url_safe("\251\255\254"))
out.round_trip_url = { url == "\251\255\254", url_err == nil }

local bare, bare_err = b64.decode_no_pad("aGk")
out.decode_no_pad = { bare, bare_err == nil }

-- a failure is `(nil, err)`; what `err` is differs (see base64.vm2.luau)
for name, input in {
	decode = "not*base64!",
	decode_no_pad = "aGVsbG8=",
	decode_url_safe = "not*base64!",
	decode_url_safe_no_pad = "aGVsbG8=",
} do
	local value, why = b64[name](input)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

return out
