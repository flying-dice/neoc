-- intentional: a decode failure is a typed error { kind = "odd_length" | "invalid_character", character?, offset? }; the offset counts skipped whitespace.
local hex = require("lib:hex")

local value, odd = hex.decode("abc")
assert(value == nil and odd.kind == "odd_length" and odd.op == "lib:hex.decode")
assert(tostring(odd):find("^lib:hex%.decode: odd_length: "))

local _, bad = hex.decode("ab  zz")
assert(bad.kind == "invalid_character" and bad.character == "z" and bad.offset == 4)

local ok, refused = pcall(hex.encode, 1)
assert(not ok and tostring(refused):find("lib:hex.encode: argument #1 'input': expected a string, got 1", 1, true))
