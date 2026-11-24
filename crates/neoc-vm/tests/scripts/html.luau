--!strict
-- lib:html as a script sees it: its answers (html.json), then what it asserts.
local html = require("lib:html")
local out = {}

out.escape = {}
for i, text in
  {
    [[<script>alert("xss")</script>]],
    "it's",
    "",
    "hello",
    "café",
    "🦀",
    "&amp;",
    "a & b < c > d",
  }
do
  out.escape[i] = html.escape(text)
end

out.unescape = {}
for i, text in
  {
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
  }
do
  out.unescape[i] = html.unescape(text)
end

out.two_passes = html.unescape(html.unescape("&amp;lt;"))
out.round_trip = {}
for i, text in { "hello", [[<>&"']], "", "&amp;", "café", "🦀" } do
  out.round_trip[i] = html.unescape(html.escape(text)) == text
end

-- what is not text is refused
out.refused = { (pcall(html.escape :: any, nil)), (pcall(html.escape :: any, {})), (pcall(html.unescape :: any, nil)) }

-- what it asserts
do
  assert(html.escape("<b>") == "&lt;b&gt;")
  for _, extra in { {}, { context = "attribute" }, { unknown = true } } do
    local ok, refused = pcall(html.escape :: any, "<b>", extra) -- deliberate refusal test: an argument too many
    assert(
      not ok
        and tostring(refused):find("lib:html.escape", 1, true)
        and tostring(refused):find("takes 1 argument", 1, true)
    )
  end

  local ok, refused = pcall(html.escape, 42 :: any) -- deliberate refusal test: a number is not text
  assert(not ok and tostring(refused):find("lib:html.escape: argument #1 'text'", 1, true))
  ok, refused = pcall(html.unescape, "\255")
  assert(not ok and tostring(refused):find("lib:html.unescape: argument #1 'text'", 1, true))
end

return out
