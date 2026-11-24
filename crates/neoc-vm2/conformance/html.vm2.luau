--!strict
-- intentional: escape takes the text alone (it took a second, ignored options table "for later"), and a wrong argument is refused with its place.
local html = require("lib:html")

assert(html.escape("<b>") == "&lt;b&gt;")
for _, extra in { {}, { context = "attribute" }, { unknown = true } } do
	local ok, refused = pcall(html.escape :: any, "<b>", extra) -- deliberate refusal test: an argument too many
	assert(not ok and tostring(refused):find("lib:html.escape", 1, true) and tostring(refused):find("takes 1 argument", 1, true))
end

local ok, refused = pcall(html.escape, 42 :: any) -- deliberate refusal test: a number is not text
assert(not ok and tostring(refused):find("lib:html.escape: argument #1 'text'", 1, true))
ok, refused = pcall(html.unescape, "\255")
assert(not ok and tostring(refused):find("lib:html.unescape: argument #1 'text'", 1, true))
