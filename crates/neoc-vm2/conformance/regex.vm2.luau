-- intentional: flags are a typed options table ({ case_insensitive = true }, not "i"), a compile failure is a typed error (syntax, too_big), a negative split limit is refused as an argument, and a Match's groups are { kind = "matched", text, start, finish } or { kind = "unmatched" } (they were strings, or false), with Pattern.source as a field.
local regex = require("lib:regex")

assert(regex.is_match("hello", "HELLO", { case_insensitive = true }))
assert(regex.is_match("^b$", "a\nb", { multi_line = true }))
assert(regex.is_match("a.b", "a\nb", { dot_matches_newline = true }))
assert(regex.is_match("a b", "ab", { ignore_whitespace = true }))
local p = assert(regex.Pattern.new("hello", { case_insensitive = true }))
assert(p:is_match("HELLO") and p.source == "hello")

local m = assert(regex.find("(\\d+)-(\\d+)", "size 12-34 px"))
assert(m.groups[1].kind == "matched" and m.groups[1].text == "12" and m.groups[1].start == 6 and m.groups[1].finish == 7)
local optional = assert(regex.find("(a)?(b)", "b"))
assert(#optional.groups == 2 and optional.groups[1].kind == "unmatched" and optional.groups[2].text == "b")

local _, syntax = regex.is_match("(abc", "x")
assert(syntax.kind == "syntax" and syntax.op == "lib:regex.is_match")
assert(tostring(syntax):find("lib:regex.is_match: syntax:", 1, true))
local _, lookaround = regex.Pattern.new("a(?=b)")
assert(lookaround.kind == "syntax" and lookaround.op == "lib:regex.Pattern.new")
local _, big = regex.Pattern.new("(?:(?:a{100}){100}){100}")
assert(big.kind == "too_big" and big.limit > 0)

local ok, refused = pcall(regex.split, ",", "a,b", -1)
assert(not ok and tostring(refused):find("argument #3 'limit'", 1, true))
local ok2, refused2 = pcall(regex.is_match, "a", "a", { ignore_case = true })
assert(not ok2 and tostring(refused2):find("argument #3 'flags'", 1, true))
