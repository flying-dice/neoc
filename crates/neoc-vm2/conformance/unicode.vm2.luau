-- intentional: the normalisation form is a "nfc" | "nfd" | "nfkc" | "nfkd" union (case-sensitive; an unknown form is refused as an argument, `normalize` cannot fail), `category` takes a validated single codepoint (refused as an argument) and returns a union of the two-letter categories without a second value.
local unicode = require("lib:unicode")

assert(select("#", unicode.normalize("x", "nfc")) == 1, "normalize returns one value")
assert(select("#", unicode.category("x")) == 1, "category returns one value")

local ok, refused = pcall(unicode.normalize, "x", "NFC")
assert(not ok and tostring(refused):find("argument #2 'form'", 1, true))
local ok2, refused2 = pcall(unicode.normalize, "x", "nfz")
assert(not ok2 and tostring(refused2):find("lib:unicode.normalize: argument #2 'form'", 1, true))

for _, bad in { "ab", "", "e\u{0301}" } do
	local ok3, why = pcall(unicode.category, bad)
	assert(not ok3 and tostring(why):find("argument #1 'ch': ", 1, true) and tostring(why):find("expected a single codepoint", 1, true))
end
assert(unicode.category("A") == "Lu" and unicode.category("\u{20AC}") == "Sc")
