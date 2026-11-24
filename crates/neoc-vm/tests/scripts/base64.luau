--!strict
-- lib:base64 as a script sees it: its answers (base64.json), then what it asserts.
local b64 = require("lib:base64")
local out = {}

out.encode = b64.encode("hello")
out.encode_empty = b64.encode("")
out.padding = { b64.encode("hi"), b64.encode_no_pad("hi") }
out.url_safe =
  { b64.encode("\251\255\254"), b64.encode_url_safe("\251\255\254"), b64.encode_url_safe_no_pad("\251\255\254") }

local raw = "\0\1\2\254\255"
local decoded, err = b64.decode(b64.encode(raw))
out.round_trip = { decoded == raw, err == nil, #assert(decoded) } :: { any }

local url, url_err = b64.decode_url_safe(b64.encode_url_safe("\251\255\254"))
out.round_trip_url = { url == "\251\255\254", url_err == nil }

local bare, bare_err = b64.decode_no_pad("aGk")
out.decode_no_pad = { bare, bare_err == nil } :: { any }

-- a failure is `(nil, err)`
for name, input in
  {
    decode = "not*base64!",
    decode_no_pad = "aGVsbG8=",
    decode_url_safe = "not*base64!",
    decode_url_safe_no_pad = "aGVsbG8=",
  } :: { [string]: string }
do
  local value, why = b64[name](input)
  out["fails_" .. name] = { value == nil, why ~= nil }
end

-- what it asserts
do
  local value, err = b64.decode("not*base64!")
  assert(value == nil and err ~= nil)
  assert(err.kind == "invalid_byte" and err.offset == 3 and err.byte == 42)
  assert(err.op == "lib:base64.decode")
  assert(tostring(err):find("^lib:base64%.decode: invalid_byte: "))

  local _, at = b64.decode("ab*d")
  assert(at ~= nil and at.kind == "invalid_byte" and at.offset == 2 and at.byte == 42)

  local _, padding = b64.decode_no_pad("aGVsbG8=")
  assert(padding ~= nil)
  assert(padding.kind == "invalid_padding", padding.kind)
  assert(padding.op == "lib:base64.decode_no_pad")

  local _, length = b64.decode("a")
  assert(length ~= nil and length.kind == "invalid_length" and length.length == 1)

  -- bytes in and out: any byte, `\0` included
  local data = assert(b64.decode("AAECgP8="))
  assert(data == "\0\1\2\128\255")

  -- a wrong argument is refused with its place
  local ok, refused = pcall(b64.encode, 1 :: any) -- deliberate refusal test: a number is not bytes
  assert(not ok and tostring(refused):find("lib:base64.encode: argument #1 'input': expected a string, got 1", 1, true))
end

return out
