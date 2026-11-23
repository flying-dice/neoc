-- lib:regex on neoc-vm and neoc-vm2: what both must answer the same (flags are written inline here).
local regex = require("lib:regex")
local out = {}

out.is_match = { (regex.is_match("\\d+", "abc 123 def")), (regex.is_match("\\d+", "abc def")) }
out.inline_flags = {
	(regex.is_match("(?i)hello", "Hello")),
	(regex.is_match("(?m)^second$", "first\nsecond")),
	(regex.is_match("^second$", "first\nsecond")),
	(regex.is_match("(?s)a.b", "a\nb")),
}
local accent = regex.find("\u{e9}", "caf\u{e9} X")
out.unicode = { (regex.is_match("^\\w+$", "caf\u{e9}")), accent.start, accent.finish }

local m = regex.find("(\\d+)-(\\d+)", "size 12-34 px")
out.find = { m.text, m.start, m.finish }
local s1, e1 = string.find("abc 123 def 456", "123", 1, true)
local found = regex.find("123", "abc 123 def 456")
out.like_string_find = s1 == found.start and e1 == found.finish
local empty = regex.find("", "abc")
out.empty_match = { empty.start, empty.finish }
local miss, miss_err = regex.find("zzz", "abc")
out.miss = { miss == nil, miss_err == nil }

local texts = {}
for i, hit in ipairs(regex.find_all("\\d+", "1 22 333 4444")) do
	texts[i] = hit.text
end
out.find_all = texts
out.find_all_none = #regex.find_all("x", "abc")

local date = regex.find("(?P<year>\\d{4})-(?P<month>\\d{2})", "date 2026-05-19 done")
out.named = { date.named.year, date.named.month }

out.replace = {
	(regex.replace("a", "aaa", "X")),
	(regex.replace_all("a", "aaa", "X")),
	(regex.replace_all("(\\w+)=(\\w+)", "a=1 b=2", "$2=$1")),
	(regex.replace_all("(?P<k>\\w+)=(?P<v>\\w+)", "x=10", "${v}=${k}")),
}
out.split = {
	(regex.split("\\s+", "  one  two   three ")),
	(regex.split(",", "a,b,c,d", 2)),
	(regex.split(",", "a,b,c", 0)),
}
out.escape = { regex.escape("a.b"), (regex.is_match(regex.escape("1.2.3+build (rc)"), "1.2.3+build (rc)")) }

local p = regex.Pattern.new("\\d+")
out.pattern = { p:is_match("nope"), #p:find_all("1 22 333"), p:find("abc 123").text, p:find("abc") == nil, tostring(p) }
out.pattern_split = regex.Pattern.new(","):split("a,b,c", 2)
out.pattern_replace = { p:replace("1 2", "N"), p:replace_all("1 2", "N") }

local bad, bad_err = regex.is_match("(abc", "x")
out.invalid = { bad == nil, bad_err ~= nil }
local bad_pattern, bad_pattern_err = regex.Pattern.new("(abc")
out.invalid_pattern = { bad_pattern == nil, bad_pattern_err ~= nil }
return out
