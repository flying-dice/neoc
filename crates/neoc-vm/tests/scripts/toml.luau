--!strict
-- vnd:toml as a script sees it: its answers (toml.json), then what it asserts.
local toml = require("vnd:toml")
local out = {}

local doc, err = toml.decode_string([=[
title = "demo"
count = 30
negative = -7
ratio = 0.5
enabled = true
nums = [1, 2, 3]
mixed = ["a", 1, true, [2, 3]]
point = { x = 1, y = 2 }
multi = """
two
lines"""
literal = 'C:\path'

[server]
host = "localhost"
port = 8080

[server.tls]
enabled = false

[[jobs]]
id = 1

[[jobs]]
id = 2
tags = ["x"]
]=])
out.decode = { doc, err == nil } :: { any }
out.empty = { toml.decode_string("") }
out.comment_only = toml.decode_string("# nothing here\n")
out.toml_1_1 = { toml.decode_string("x = {\n  a = 1,\n  b = 2,\n}\n"), toml.decode_string('x = "\\e"') }
out.unicode = toml.decode_string('"キー" = "値"\n')

-- a failure is `(nil, err)`
for name, text in
  {
    unclosed = "key = [unclosed",
    duplicate = "a = 1\na = 2",
    no_value = "a =",
    deep = "a = " .. ("["):rep(100000) .. ("]"):rep(100000),
  } :: { [string]: string }
do
  local value, why = toml.decode_string(text)
  out["fails_" .. name] = { value == nil, why ~= nil }
end

local config = {
  name = "bob",
  age = 25,
  ratio = 1.5,
  on = true,
  tags = { "a", "b" },
  server = { host = "localhost", port = 3000, limits = { cpu = 2 } },
  jobs = { { id = 1 }, { id = 2 } },
}
out.encode = { toml.encode(config) }
out.encode_pretty = { toml.encode_pretty(config) }
out.encode_empty = toml.encode {}
out.round_trip = toml.decode_string((assert(toml.encode(config))))

-- a std:io reader gives what its text gives (a writer over a file is in files.luau)
local io = require("std:io")
out.reader = { toml.decode(io.from_string('title = "demo"\nnums = [1, 2]\n\n[server]\nport = 8080\n')) }
out.reader_empty = { toml.decode(io.from_string("")) }
local read, read_why = toml.decode(io.from_string("key = [unclosed"))
out.reader_fails = { read == nil, read_why ~= nil }

-- what it asserts
do
  -- dates and times are text
  local doc = assert(toml.decode_string("t = 07:32\nd = 1979-05-27\nat = 1979-05-27T07:32:00Z"))
  assert(doc.t == "07:32" and doc.d == "1979-05-27" and doc.at == "1979-05-27T07:32:00Z")

  -- a std:io reader
  local read = assert(toml.decode(io.from_string("at = 1979-05-27\n")))
  assert(read.at == "1979-05-27")
  local nothing, read_syntax = toml.decode(io.from_string("ok = 1\nbad = [unclosed"))
  assert(nothing == nil and read_syntax ~= nil)
  assert(read_syntax.kind == "syntax" and read_syntax.line == 2 and read_syntax.op == "vnd:toml.decode")
  local _, not_utf8 = toml.decode(io.from_string("a = 1\n\255"))
  assert(not_utf8 ~= nil and not_utf8.kind == "invalid_utf8" and not_utf8.offset == 6)
  local not_reader_ok, not_reader = pcall(function()
    toml.decode("a = 1" :: any) -- deliberate refusal test: text is not a reader
  end)
  assert(
    not not_reader_ok and tostring(not_reader):find("vnd:toml.decode: argument #1 'reader': expected a Reader", 1, true)
  )
  local not_writer_ok, not_writer = pcall(function()
    toml.encode_to({ a = 1 }, io.from_string("") :: any) -- deliberate refusal test: a reader is not a writer
  end)
  assert(
    not not_writer_ok
      and tostring(not_writer):find("vnd:toml.encode_to: argument #2 'writer': expected a Writer", 1, true)
  )

  -- typed errors
  local _, syntax = toml.decode_string("ok = 1\nbad = [unclosed")
  assert(syntax ~= nil)
  assert(syntax.kind == "syntax" and syntax.line == 2 and syntax.column > 1 and syntax.op == "vnd:toml.decode_string")
  assert(
    type(syntax.reason) == "string"
      and tostring(syntax):find("vnd:toml.decode_string: syntax: line 2, column ", 1, true)
  )
  local text, unsupported = toml.encode { n = 0 / 0 }
  assert(text == nil and unsupported ~= nil)
  assert(unsupported.kind == "unsupported" and unsupported.op == "vnd:toml.encode")

  -- a document is a table with string keys
  local encoders: { [string]: ({ [string]: any }) -> (string?, vnd_toml_EncodeError?) } = {
    encode = toml.encode,
    encode_pretty = toml.encode_pretty,
  }
  -- deliberate refusal test: none is a document
  for _, value in
    { "text", 42, { 1, 2, 3 }, { f = print }, { data = buffer.create(2) }, { nested = { data = buffer.create(2) } } } :: { any }
  do
    for name, encode in encoders do
      local ok, refused = pcall(function()
        encode(value)
      end)
      assert(not ok and tostring(refused):find("vnd:toml." .. name .. ": argument #1 'value'", 1, true))
    end
  end
  assert(not pcall(function()
    toml.decode_string(42 :: any) -- deliberate refusal test: a number is not text
  end))
end

return out
