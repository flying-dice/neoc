-- lib:html on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- escape's ignored options table is gone on purpose: see html.vm2.luau.
local html = require("lib:html")
local out = {}

out.escape = {}
for i, text in {
	[[<script>alert("xss")</script>]],
	"it's",
	"",
	"hello",
	"café",
	"🦀",
	"&amp;",
	"a & b < c > d",
} do
	out.escape[i] = html.escape(text)
end

out.unescape = {}
for i, text in {
	"&amp;&lt;&gt;&quot;&apos;",
	"&lt;b&gt;bold&lt;/b&gt;",
	"&notareal;",
	"&copy;",
	"&amp",
	"& amp;",
	"foo&",
	"&#65;",
	"&#x41;",
	"&#X41;",
	"&#x41;&#65;",
	"&#0;",
	"&#xD800;",
	"&#1114112;",
	"&#999999;",
	"&#x10FFFF;",
	"&#x110000;",
	"&#;",
	"&#x;",
	"&#xGG;",
	"&#+5;",
	"&#x+5;",
	"&abcdefghijk;",
	"&ÜÜÜÜÜÜÜÜ;",
	"AT&T Ünïcödé 🦀🦀🦀 &amp; &#x1F980;",
	"&amp;lt;",
	"",
} do
	out.unescape[i] = html.unescape(text)
end

out.two_passes = html.unescape(html.unescape("&amp;lt;"))
out.round_trip = {}
for i, text in { "hello", [[<>&"']], "", "&amp;", "café", "🦀" } do
	out.round_trip[i] = html.unescape(html.escape(text)) == text
end

-- what is not text is refused
out.refused = { (pcall(html.escape, nil)), (pcall(html.escape, {})), (pcall(html.unescape, nil)) }

return out
