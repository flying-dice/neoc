-- lib:hex on neoc-vm and neoc-vm2: what both must answer the same.
local hex = require("lib:hex")
local out = {}

out.encode = { hex.encode("\255\171"), hex.encode_upper("\255\171"), hex.encode(""), hex.encode_upper("") }
out.decode = { hex.decode("6869"), hex.decode(""), hex.decode("fFaB") == "\255\171" }
out.whitespace = hex.decode("de ad\tbe\nef") == "\222\173\190\239"

local raw = "\0\1\2\254\255"
local decoded, err = hex.decode(hex.encode(raw))
out.round_trip = { decoded == raw, err == nil, #decoded }

for name, input in { odd = "abc", non_hex = "zz" } do
	local value, why = hex.decode(input)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

return out
