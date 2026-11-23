-- intentional: counts are unsigned and refused as arguments when negative or fractional, the pad fill and the `boundary` option are validated types (refused as arguments, not returned errors), `closest` returns an empty list instead of nil and cannot fail, `truncate` cannot fail, and failures are typed error tables.
local text = require("lib:text")

local _, zero = text.word_wrap("hi", 0)
assert(zero.kind == "zero_width" and zero.op == "lib:text.word_wrap")
assert(tostring(zero) == "lib:text.word_wrap: zero_width: width must be greater than 0")

local _, wide = text.pad_left("x", 4000000000)
assert(wide.kind == "too_large" and wide.requested == 4000000000 and wide.max == 1073741824)

assert(#text.closest("x", {}) == 0)
assert(text.truncate("hello world", 8) == "hello...")

for _, bad in { "ab", "", "あ" } do
	local ok, refused = pcall(text.pad_left, "x", 5, bad)
	assert(not ok and tostring(refused):find("argument #3", 1, true), tostring(refused))
end
local ok, refused = pcall(text.clip, "x", -1)
assert(not ok and tostring(refused):find("argument #2 'max'", 1, true))
local ok2, refused2 = pcall(text.truncate, "hello", 4, { boundary = "line" })
assert(not ok2 and tostring(refused2):find("argument #3 'opts'", 1, true))
