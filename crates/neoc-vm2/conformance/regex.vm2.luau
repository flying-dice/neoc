--!strict
-- intentional: flags are a typed options table ({ case_insensitive = true }, not "i"), a compile failure is a typed error (syntax, too_big), a negative split limit is refused as an argument, and a Match's groups are { kind = "matched", text, start, finish } or { kind = "unmatched" } (they were strings, or false), with Pattern.source as a field.
local regex = require("lib:regex")

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
assert(#optional.groups == 2 and optional.groups[1].kind == "unmatched" and second.kind == "matched" and second.text == "b")

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
