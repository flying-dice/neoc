--!strict
-- lib:text as a script sees it: its answers (text.json), then what it asserts.
local text = require("lib:text")
local out = {}

out.camel = text.to_camel_case("hello_world")
out.snake = text.to_snake_case("HTTPSConnection")
out.kebab = text.to_kebab_case("helloWorld")
out.pascal = text.to_pascal_case("hello world")
out.title = text.to_title_case("helloWorld")
out.constant = text.to_constant_case("helloWorld")

out.wrap = (text.word_wrap("hello world", 5))
out.wrap_overflow = (text.word_wrap("abcdefghij", 5))
out.wrap_break = (text.word_wrap("abcdefghij", 5, { break_words = true }))
out.wrap_flat = (text.word_wrap("line1\nline2", 80, { preserve_newlines = false }))
out.wrap_wide = (text.word_wrap("世界 你", 5))
local wrapped, wrap_err = text.word_wrap("hi", 0)
out.wrap_zero = { wrapped == nil, wrap_err ~= nil }

out.levenshtein = text.levenshtein("kitten", "sitting")
local near = text.closest("teh", { "the", "tea", "ten" })
out.closest = { near[1].match, near[2].match, near[3].match, near[1].distance, near[3].distance } :: { any }
out.closest_limit = #text.closest("teh", { "the", "tea", "ten" }, { limit = 2 })

out.truncate = { (text.truncate("hello world", 8)), (text.truncate("hello world", 8, { boundary = "word" })) }
out.truncate_short = { (text.truncate("hi", 8)), (text.truncate("hello", 2)), (text.truncate("hello", 3)) }

out.pad = {
  (text.pad_left("42", 5)),
  (text.pad_left("42", 5, "0")),
  (text.pad_right("name", 10)),
  (text.center("title", 11)),
  (text.center("ab", 5)),
  (text.pad_left("世", 4, ".")),
}

out.slug = text.slugify("Café résumé")
out.trim = { text.trim("\u{3000} a b \u{a0}\n"), text.trim_start("  a "), text.trim_end("  a ") }
out.clip = {
  text.clip("hello world", 8, "..."),
  text.clip("hi", 8, "..."),
  text.clip("abcdef", 2, "..."),
  text.clip("hello", 0),
}
out.valid = { text.to_valid_utf8("a\255b") == "a\u{FFFD}b", text.to_valid_utf8("fine") } :: { any }
out.ansi = { text.strip_ansi("\27[31mred\27[0m"), text.strip_ansi("a\r\nb\tc"), text.strip_ansi("a\27]0;never\nb") }

-- what it asserts
do
  local _, zero = text.word_wrap("hi", 0)
  assert(zero ~= nil and zero.kind == "zero_width" and zero.op == "lib:text.word_wrap")
  assert(tostring(zero) == "lib:text.word_wrap: zero_width: width must be greater than 0")

  local _, wide = text.pad_left("x", 4000000000)
  assert(wide ~= nil and wide.kind == "too_large" and wide.requested == 4000000000 and wide.max == 1073741824)

  assert(#text.closest("x", {}) == 0)
  assert(text.truncate("hello world", 8) == "hello...")

  for _, bad in { "ab", "", "あ" } do
    local ok, refused = pcall(function()
      text.pad_left("x", 5, bad)
    end)
    assert(not ok and tostring(refused):find("argument #3", 1, true), tostring(refused))
  end
  local ok, refused = pcall(text.clip, "x", -1)
  assert(not ok and tostring(refused):find("argument #2 'max'", 1, true))
  local ok2, refused2 = pcall(function()
    return text.truncate("hello", 4, { boundary = "line" } :: any) -- deliberate refusal test: not a boundary
  end)
  assert(not ok2 and tostring(refused2):find("argument #3 'opts'", 1, true))
end

return out
