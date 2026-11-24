--!strict
-- vnd:serde_yaml as a script sees it: its answers (serde_yaml.json), then what it asserts.
local serde_yaml = require("vnd:serde_yaml")
local out = {}

local doc, err = serde_yaml.decode_string([=[
title: demo
count: 30
negative: -7
ratio: 0.5
enabled: true
nums: [1, 2, 3]
mixed: [a, 1, true, [2, 3]]
point: { x: 1, y: 2 }
multi: |
  two
  lines
folded: >
  one
  line
quoted: "tab\there"
server:
  host: localhost
  port: 8080
  tls:
    enabled: false
jobs:
  - id: 1
  - id: 2
    tags: [x]
]=])
out.decode = { doc, err == nil }
out.sequence = serde_yaml.decode_string("- a\n- b\n")
out.scalars =
  { serde_yaml.decode_string("hello\n"), serde_yaml.decode_string("true\n"), serde_yaml.decode_string("42") }
out.flattened =
  serde_yaml.decode_string("1: one\ntrue: yes\nat: 2001-12-14t21:59:43.10-05:00\nhex: 0x10\nbase: &x [1]\ncopy: *x\n")
out.unicode = serde_yaml.decode_string("キー: 値\n")

-- a failure is `(nil, err)`
for name, text in
  {
    unclosed = "key: [unclosed",
    bad_indent = "a:\n  b: 1\n c: 2",
    no_anchor = "a: *nope",
    tagged = "a: !Tag {b: 1}",
    several = "a: 1\n---\nb: 2",
    not_utf8 = "a: \255",
    deep = ("["):rep(100000) .. ("]"):rep(100000),
  } :: { [string]: string }
do
  local value, why = serde_yaml.decode_string(text)
  out["fails_" .. name] = { value == nil, why ~= nil }
end

local config = {
  name = "bob",
  age = 25,
  ratio = 1.5,
  on = true,
  text = "two\nlines",
  tags = { "a", "b" },
  server = { host = "localhost", port = 3000, limits = { cpu = 2 } },
  jobs = { { id = 1 }, { id = 2 } },
}
out.encode = { serde_yaml.encode(config) }
out.encode_scalars =
  { serde_yaml.encode(42), serde_yaml.encode(true), serde_yaml.encode("hello"), serde_yaml.encode(nil) }
out.encode_list = serde_yaml.encode({ 1, "two", { 3 } } :: { any })
out.round_trip = serde_yaml.decode_string((assert(serde_yaml.encode(config))))

-- a std:io reader gives what its text gives (a writer over a file is in files.luau)
local io = require("std:io")
out.reader = { serde_yaml.decode(io.from_string("name: alice\nnums: [1, 2]\ndb:\n  host: localhost\n")) }
local read, read_why = serde_yaml.decode(io.from_string("key: [unclosed"))
out.reader_fails = { read == nil, read_why ~= nil }

-- what it asserts
do
  -- null is nil
  local doc = assert(serde_yaml.decode_string("key: null\nother: ~\nkept: 1\nnan: .nan\n"))
  assert(doc.key == nil and doc.other == nil and doc.nan == nil and doc.kept == 1)
  for _, text in { "", "~", "null" } do
    local value, none = serde_yaml.decode_string(text)
    assert(value == nil and none == nil)
  end

  -- a std:io reader
  local read = assert(serde_yaml.decode(io.from_string("key: null\nkept: 1\n")))
  assert(read.key == nil and read.kept == 1)
  local nothing, read_syntax = serde_yaml.decode(io.from_string("key: [unclosed"))
  assert(nothing == nil and read_syntax ~= nil)
  assert(read_syntax.kind == "syntax" and read_syntax.op == "vnd:serde_yaml.decode")
  local not_reader_ok, not_reader = pcall(function()
    serde_yaml.decode("a: 1" :: any) -- deliberate refusal test: text is not a reader
  end)
  assert(
    not not_reader_ok
      and tostring(not_reader):find("vnd:serde_yaml.decode: argument #1 'reader': expected a Reader", 1, true)
  )
  local not_writer_ok, not_writer = pcall(function()
    serde_yaml.encode_to({ a = 1 }, io.from_string("") :: any) -- deliberate refusal test: a reader is not a writer
  end)
  assert(
    not not_writer_ok
      and tostring(not_writer):find("vnd:serde_yaml.encode_to: argument #2 'writer': expected a Writer", 1, true)
  )

  -- typed errors
  local _, syntax = serde_yaml.decode_string("ok: 1\nkey: *nope")
  assert(syntax ~= nil)
  assert(syntax.kind == "syntax" and syntax.line == 2 and syntax.column == 6 and syntax.reason == "unknown anchor")
  assert(syntax.op == "vnd:serde_yaml.decode_string")
  assert(tostring(syntax) == "vnd:serde_yaml.decode_string: syntax: line 2, column 6: unknown anchor")
  local _, tagged = serde_yaml.decode_string("!Tag x")
  assert(tagged ~= nil and tagged.kind == "syntax" and tagged.line == 1)
  local _, several = serde_yaml.decode_string("a: 1\n---\nb: 2")
  assert(several ~= nil and several.kind == "invalid" and several.reason:find("more than one document", 1, true))

  -- what is not plain data is refused as an argument
  local cyclic: { [string]: any } = {}
  cyclic.self = cyclic
  local mixed: { any } = { 1, buffer.create(2) }
  local not_data: { any } = { print, { f = print }, buffer.create(2), { list = mixed }, cyclic }
  for _, bad in not_data do
    local ok, refused = pcall(function()
      serde_yaml.encode(bad)
    end)
    assert(not ok and tostring(refused):find("vnd:serde_yaml.encode: argument #1 'value'", 1, true))
  end
  assert(not pcall(function()
    serde_yaml.decode_string(42 :: any) -- deliberate refusal test: a number is not text
  end))

  -- a number that is not finite is written null
  assert(serde_yaml.encode { n = 0 / 0 } == "n: null\n")
end

return out
