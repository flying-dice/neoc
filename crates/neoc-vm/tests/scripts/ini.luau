--!strict
-- lib:ini as a script sees it: its answers (ini.json), then what it asserts.
local ini = require("lib:ini")
local out = {}

local doc, err =
  ini.parse("name=world\n; a comment\n# another\n\n[server]\nhost = localhost\nport=5432\nverbose\nempty=\n")
out.parse = { doc, err == nil } :: { any }

out.duplicates = ini.parse("[s]\nk=a\nk=b\n[t]\nx=1\n[s]\nother=2")
out.no_inline_comments = ini.parse("[s]\nkey=value ; not a comment")
out.empty_section = ini.parse("[empty]\n[other]\nk=v")
out.unicode = ini.parse("[日本語]\nキー=値")
out.colons = ini.parse("[a:b]\nurl=http://host:80/x\nbare:key")
out.quoted = ini.parse('[s]\na="quoted value"\nb=\'single\'\nc=plain "inner" text')
out.whitespace = ini.parse("  [ s ]  \n   key   =   spaced value   \n\tflag\t")
out.crlf = ini.parse("[s]\r\nkey=value\r\nflag\r\n")
out.nothing = { ini.parse(""), ini.parse("; comment\n# another") }

out.lowercase = ini.parse("[Section]\nKey=Val\nFLAG", { lowercase_keys = true })
out.folded = ini.parse("[Foo]\na=1\n[FOO]\nB=2", { lowercase_keys = true })
out.comment_symbols = ini.parse("!ignored\nkey=val", { comment_symbols = { "!" } })
out.delimiters = ini.parse("[s]\nkey:val\nother: x:y", { delimiters = { ":" } })
out.both_delimiters = ini.parse("a=1:2\nb:3=4", { delimiters = { "=", ":" } })

-- a failure is `(nil, err)`
for name, text in
  {
    unclosed = "[bad",
    empty_name = "[]",
    control = "key=val\0ue",
    sentinel = "[s]\nkey\2=value",
  } :: { [string]: string }
do
  local value, why = ini.parse(text)
  out["fails_" .. name] = { value == nil, why ~= nil }
end

-- one key per section, so that the order of a section's keys is not part of the answer
out.stringify = {
  ini.stringify { section = { key = "value" } },
  ini.stringify { section = { flag = true } },
  ini.stringify { section = { off = false } },
  ini.stringify { section = { n = 42 } },
  ini.stringify { section = { n = 1.5 } },
  ini.stringify { [""] = { name = "world" }, section = { k = "v" } },
  ini.stringify({ section = { key = "value" } }, { spacing = true }),
  ini.stringify({ section = { key = "value" } }, { spacing = false }),
  ini.stringify {},
}
out.round_trip = ini.stringify((assert(ini.parse("top=1\n\n[db]\nhost=localhost\n"))))
out.stringify_refuses =
  { (pcall(ini.stringify :: any, { s = { bad = {} } })), (pcall(ini.stringify :: any, { s = { bad = print } })) }

-- what it asserts
do
  -- typed errors
  local _, unclosed = ini.parse("a=1\n[bad")
  assert(
    unclosed ~= nil and unclosed.kind == "unclosed_section" and unclosed.line == 2 and unclosed.op == "lib:ini.parse"
  )
  assert(tostring(unclosed) == "lib:ini.parse: unclosed_section: line 2: unclosed section header")
  local _, empty = ini.parse("[]")
  assert(empty ~= nil and empty.kind == "empty_section_name" and empty.line == 1)
  local _, control = ini.parse("key=val\0ue")
  assert(control ~= nil and control.kind == "control_byte" and control.offset == 7 and control.byte == 0)

  -- stringify: one value, sorted
  local written =
    table.pack(ini.stringify { zeta = { k = "v" }, [""] = { name = "world" }, alpha = { b = 2, a = "1", c = true } })
  assert(written[1] == "name=world\n\n[alpha]\na=1\nb=2\nc\n\n[zeta]\nk=v\n" and written[2] == nil)

  -- symbols are whole strings, and the defaults they replace are plain characters
  local doc = assert(ini.parse("// note\n/ not=a comment\n;x=1\n#y", { comment_symbols = { "//" } }))
  assert(doc[""]["/ not"] == "a comment" and doc[""][";x"] == "1" and doc[""]["#y"] == true)
  local arrows = assert(ini.parse("a => 1\nb: 2 => 3\nc=4", { delimiters = { "=>", ":" } }))
  assert(arrows[""].a == "1" and arrows[""].b == "2 => 3" and arrows[""]["c=4"] == true)

  -- arguments are checked by their types
  -- deliberate refusal tests: the casts are options and a value of the wrong shape
  local ok, refused = pcall(function()
    ini.parse("a=1", { lowercase = true } :: any)
  end)
  assert(not ok and tostring(refused):find("lib:ini.parse: argument #2 'opts'", 1, true))
  assert(not pcall(function()
    ini.parse("a=1", { delimiters = "=" } :: any)
  end))
  local ok2, refused2 = pcall(ini.stringify, { s = { bad = {} } } :: any)
  assert(not ok2 and tostring(refused2):find("lib:ini.stringify: argument #1 'config'", 1, true))
  assert(not pcall(function()
    ini.stringify({ s = { k = "v" } }, { spacing = "yes" } :: any)
  end))
end

return out
