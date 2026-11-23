-- intentional: escape takes the text alone (it took a second, ignored options table "for later"), and a wrong argument is refused with its place.
local html = require("lib:html")

assert(html.escape("<b>") == "&lt;b&gt;")
for _, extra in { {}, { context = "attribute" }, { unknown = true } } do
	local ok, refused = pcall(html.escape, "<b>", extra)
	assert(not ok and tostring(refused):find("lib:html.escape", 1, true) and tostring(refused):find("takes 1 argument", 1, true))
end

local ok, refused = pcall(html.escape, 42)
assert(not ok and tostring(refused):find("lib:html.escape: argument #1 'text'", 1, true))
ok, refused = pcall(html.unescape, "\255")
assert(not ok and tostring(refused):find("lib:html.unescape: argument #1 'text'", 1, true))
