--!strict
-- lib:hex as a script sees it: its answers (hex.json), then what it asserts.
local hex = require("lib:hex")
local out = {}

out.encode = { hex.encode("\255\171"), hex.encode_upper("\255\171"), hex.encode(""), hex.encode_upper("") }
out.decode = { hex.decode("6869"), hex.decode(""), hex.decode("fFaB") == "\255\171" } :: { any }
out.whitespace = hex.decode("de ad\tbe\nef") == "\222\173\190\239"

local raw = "\0\1\2\254\255"
local decoded, err = hex.decode(hex.encode(raw))
out.round_trip = { decoded == raw, err == nil, #assert(decoded) } :: { any }

for name, input in { odd = "abc", non_hex = "zz" } :: { [string]: string } do
  local value, why = hex.decode(input)
  out["fails_" .. name] = { value == nil, why ~= nil }
end

-- what it asserts
do
  local value, odd = hex.decode("abc")
  assert(value == nil and odd ~= nil and odd.kind == "odd_length" and odd.op == "lib:hex.decode")
  assert(tostring(odd):find("^lib:hex%.decode: odd_length: "))

  local _, bad = hex.decode("ab  zz")
  assert(bad ~= nil and bad.kind == "invalid_character" and bad.character == "z" and bad.offset == 4)

  local ok, refused = pcall(hex.encode, 1 :: any) -- deliberate refusal test: a number is not bytes
  assert(not ok and tostring(refused):find("lib:hex.encode: argument #1 'input': expected a string, got 1", 1, true))
end

return out
