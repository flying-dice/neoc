--!strict
-- lib:regex as a script sees it: its answers (regex.json), then what it asserts.
local regex = require("lib:regex")
local out = {}

out.is_match = { (regex.is_match("\\d+", "abc 123 def")), (regex.is_match("\\d+", "abc def")) }
out.inline_flags = {
  (regex.is_match("(?i)hello", "Hello")),
  (regex.is_match("(?m)^second$", "first\nsecond")),
  (regex.is_match("^second$", "first\nsecond")),
  (regex.is_match("(?s)a.b", "a\nb")),
}
local accent = assert(regex.find("\u{e9}", "caf\u{e9} X"))
out.unicode = { (regex.is_match("^\\w+$", "caf\u{e9}")), accent.start, accent.finish } :: { any }

local m = assert(regex.find("(\\d+)-(\\d+)", "size 12-34 px"))
out.find = { m.text, m.start, m.finish } :: { any }
local s1, e1 = string.find("abc 123 def 456", "123", 1, true)
local found = assert(regex.find("123", "abc 123 def 456"))
out.like_string_find = s1 == found.start and e1 == found.finish
local empty = assert(regex.find("", "abc"))
out.empty_match = { empty.start, empty.finish }
local miss, miss_err = regex.find("zzz", "abc")
out.miss = { miss == nil, miss_err == nil }

local texts = {}
for i, hit in ipairs((assert(regex.find_all("\\d+", "1 22 333 4444")))) do
  texts[i] = hit.text
end
out.find_all = texts
out.find_all_none = #assert(regex.find_all("x", "abc"))

local date = assert(regex.find("(?P<year>\\d{4})-(?P<month>\\d{2})", "date 2026-05-19 done"))
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
out.escape = { regex.escape("a.b"), (regex.is_match(regex.escape("1.2.3+build (rc)"), "1.2.3+build (rc)")) } :: { any }

local p = assert(regex.Pattern.new("\\d+"))
out.pattern = {
  p:is_match("nope"),
  #p:find_all("1 22 333"),
  assert(p:find("abc 123")).text,
  p:find("abc") == nil,
  tostring(p),
} :: { any }
out.pattern_split = assert(regex.Pattern.new(",")):split("a,b,c", 2)
out.pattern_replace = { p:replace("1 2", "N"), p:replace_all("1 2", "N") }

local bad, bad_err = regex.is_match("(abc", "x")
out.invalid = { bad == nil, bad_err ~= nil }
local bad_pattern, bad_pattern_err = regex.Pattern.new("(abc")
out.invalid_pattern = { bad_pattern == nil, bad_pattern_err ~= nil }

-- what it asserts
do
  assert(regex.is_match("hello", "HELLO", { case_insensitive = true }))
  assert(regex.is_match("^b$", "a\nb", { multi_line = true }))
  assert(regex.is_match("a.b", "a\nb", { dot_matches_newline = true }))
  assert(regex.is_match("a b", "ab", { ignore_whitespace = true }))
  local p = assert(regex.Pattern.new("hello", { case_insensitive = true }))
  assert(p:is_match("HELLO") and p.source == "hello")

  local m = assert(regex.find("(\\d+)-(\\d+)", "size 12-34 px"))
  local first = m.groups[1]
  assert(first.kind == "matched" and first.text == "12" and first.start == 6 and first.finish == 7)
  local optional = assert(regex.find("(a)?(b)", "b"))
  local second = optional.groups[2]
  assert(
    #optional.groups == 2 and optional.groups[1].kind == "unmatched" and second.kind == "matched" and second.text == "b"
  )

  local _, syntax = regex.is_match("(abc", "x")
  assert(syntax ~= nil and syntax.kind == "syntax" and syntax.op == "lib:regex.is_match")
  assert(tostring(syntax):find("lib:regex.is_match: syntax:", 1, true))
  local _, lookaround = regex.Pattern.new("a(?=b)")
  assert(lookaround ~= nil and lookaround.kind == "syntax" and lookaround.op == "lib:regex.Pattern.new")
  local _, big = regex.Pattern.new("(?:(?:a{100}){100}){100}")
  assert(big ~= nil and big.kind == "too_big" and big.limit > 0)

  local ok, refused = pcall(function()
    regex.split(",", "a,b", -1)
  end)
  assert(not ok and tostring(refused):find("argument #3 'limit'", 1, true))
  local ok2, refused2 = pcall(function()
    regex.is_match("a", "a", { ignore_case = true } :: any) -- deliberate refusal test: not a flag
  end)
  assert(not ok2 and tostring(refused2):find("argument #3 'flags'", 1, true))
end

return out
