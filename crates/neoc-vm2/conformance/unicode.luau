-- lib:unicode on neoc-vm and neoc-vm2: what both must answer the same.
local unicode = require("lib:unicode")
local out = {}

out.nfc = unicode.normalize("e\u{0301}", "nfc") == "\u{e9}"
out.nfd = #unicode.normalize("\u{e9}", "nfd")
out.nfkc = unicode.normalize("\u{FB01}", "nfkc")
out.nfkd = unicode.normalize("\u{2460}", "nfkd")

out.fold = { unicode.fold("MASSE"), unicode.fold("STRASSE") == unicode.fold("stra\u{df}e") }
out.caseless = { unicode.caseless_eq("\u{c9}", "e\u{0301}"), unicode.caseless_eq("apple", "orange") }

local graphemes = unicode.graphemes("a\u{1F44D}\u{1F3FD}b")
out.graphemes = { #graphemes, graphemes[1], graphemes[3] }
out.grapheme_count = { unicode.grapheme_count("a\u{1F44D}\u{1F3FD}b"), unicode.grapheme_count("") }

local categories = {}
for _, ch in { "A", "a", "1", " ", "-", "$", "+", "\0", "\u{2163}", "\u{4E00}" } do
	categories[#categories + 1] = unicode.category(ch)
end
out.categories = categories

out.alphabetic = { unicode.is_alphabetic("h\u{e9}llo"), unicode.is_alphabetic("abc1"), unicode.is_alphabetic("") }
out.numeric = { unicode.is_numeric("123"), unicode.is_numeric("12a"), unicode.is_numeric(""), unicode.is_numeric("\u{4E00}") }
out.width = { unicode.width("abc"), unicode.width("\u{4E16}\u{754C}"), unicode.width(""), unicode.width("e\u{0301}") }
return out
