--!strict
-- vnd:serde_json as a script sees it: its answers (serde_json.json), then what it asserts.
local serde_json = require("vnd:serde_json")
local out = {}

local parsed, err =
  serde_json.from_str('{"name":"alice","age":30,"ratio":0.5,"on":true,"tags":["a","b"],"user":{"id":7}}')
local object = parsed :: lib_json_Object
out.object = {
  err == nil,
  object:get("name"),
  object:get("age"),
  object:get("ratio"),
  object:get("on"),
  (object:get("tags") :: lib_json_Array):len(),
  (object:get("tags") :: lib_json_Array):get(2),
  (object:get("user") :: lib_json_Object):get("id"),
  object:keys(),
} :: { any }
local array = assert(serde_json.from_str(' [1, "two", false, [3], {"k": "v"}] ')) :: lib_json_Array
out.array = {
  array:len(),
  array:get(1),
  array:get(2),
  array:get(3),
  (array:get(4) :: lib_json_Array):get(1),
  (array:get(5) :: lib_json_Object):get("k"),
} :: { any }
out.scalars = {
  serde_json.from_str('"hello"'),
  serde_json.from_str("42"),
  serde_json.from_str("-1.5e3"),
  serde_json.from_str("true"),
  serde_json.from_str("false"),
  serde_json.from_str('"\\u00e9\\n"'),
}
out.to_table = (serde_json.from_str('{"a":[1,2,{"b":"c"}],"d":"e"}') :: lib_json_Object):to_table()

-- a failure is `(nil, err)`
for name, text in
  {
    bad_key = "{bad}",
    empty = "",
    unclosed = "[1, 2",
    trailing = "1 2",
    not_utf8 = '"\255"',
    deep = ("["):rep(100000) .. ("]"):rep(100000),
  } :: { [string]: string }
do
  local value, why = serde_json.from_str(text)
  out["fails_" .. name] = { value == nil, why ~= nil }
end

local value = {
  name = "bob",
  age = 25,
  ratio = 1.5,
  on = true,
  text = 'quote " and \n newline é',
  scores = { 1, 2, 3 },
  nested = { deep = { list = { "x", { y = 1 } } :: { any } } },
  empty = {},
}
out.to_string = { serde_json.to_string(value) }
out.to_string_pretty = { serde_json.to_string_pretty(value) }
out.scalars_out = {
  serde_json.to_string(42),
  serde_json.to_string(-0.25),
  serde_json.to_string(1e20),
  serde_json.to_string("hello"),
  serde_json.to_string(true),
  serde_json.to_string(nil),
  serde_json.to_string {},
  serde_json.to_string({ 1, "two" } :: { any }),
}

-- a handle is written as what it holds
(object:get("tags") :: lib_json_Array):push("c")
out.handle = { serde_json.to_string(object), serde_json.to_string(object:get("tags")) }
out.round_trip = (serde_json.from_str((assert(serde_json.to_string(value)))) :: lib_json_Object):to_table()

-- 127 levels are written and read back
local function nested(depth)
  local v = {}
  for _ = 2, depth do
    v = { v }
  end
  return v
end
local text = assert(serde_json.to_string(nested(127)))
out.deepest = { #text, serde_json.from_str(text) ~= nil } :: { any }

-- a std:io reader gives what its text gives (a writer over a file is in files.luau)
local io = require("std:io")
local read = assert(serde_json.from_reader(io.from_string('{"name":"alice","tags":["a","b"]}'))) :: lib_json_Object
out.reader = {
  read:get("name"),
  (read:get("tags") :: lib_json_Array):get(2),
  serde_json.from_reader(io.from_string("42")),
  serde_json.from_reader(io.from_string('"text"')),
}
for name, text in { syntax = "{bad}", empty = "", trailing = "1 2" } :: { [string]: string } do
  local value, why = serde_json.from_reader(io.from_string(text))
  out["reader_fails_" .. name] = { value == nil, why ~= nil }
end

-- what it asserts
do
  -- null is nil, with no error
  local value, none = serde_json.from_str("null")
  assert(value == nil and none == nil)
  local doc = assert(serde_json.from_str('{"gone":null}'))
  assert(typeof(doc) == "lib_json_Object")
  assert(doc:get("gone") == nil and doc:kind("gone") == "null" and doc:has("gone"))

  -- a std:io reader
  local read_null, read_none = serde_json.from_reader(io.from_string("null"))
  assert(read_null == nil and read_none == nil)
  local nothing, read_syntax = serde_json.from_reader(io.from_string('{"ok": 1,\n bad}'))
  assert(nothing == nil and read_syntax ~= nil)
  assert(read_syntax.kind == "syntax" and read_syntax.line == 2 and read_syntax.op == "vnd:serde_json.from_reader")
  local _, read_eof = serde_json.from_reader(io.from_string(""))
  assert(read_eof ~= nil and read_eof.kind == "eof")
  local not_reader_ok, not_reader = pcall(function()
    serde_json.from_reader("1" :: any) -- deliberate refusal test: text is not a reader
  end)
  assert(
    not not_reader_ok
      and tostring(not_reader):find("vnd:serde_json.from_reader: argument #1 'reader': expected a Reader", 1, true)
  )
  local not_writer_ok, not_writer = pcall(function()
    serde_json.to_writer({ a = 1 }, io.from_string("") :: any) -- deliberate refusal test: a reader is not a writer
  end)
  assert(
    not not_writer_ok
      and tostring(not_writer):find("vnd:serde_json.to_writer: argument #2 'writer': expected a Writer", 1, true)
  )

  -- typed errors
  local _, syntax = serde_json.from_str('{"ok": 1,\n bad}')
  assert(syntax ~= nil)
  assert(
    syntax.kind == "syntax" and syntax.line == 2 and syntax.column == 2 and syntax.reason == "key must be a string"
  )
  assert(syntax.op == "vnd:serde_json.from_str")
  assert(tostring(syntax) == "vnd:serde_json.from_str: syntax: line 2, column 2: key must be a string")
  for _, text in { "", '{"a":', "[1, 2", '"open' } do
    local _, eof = serde_json.from_str(text)
    assert(eof ~= nil and eof.kind == "eof" and eof.line == 1)
  end

  local function nested(depth: number): { any }
    local v: { any } = {}
    for _ = 2, depth do
      v = { v }
    end
    return v
  end
  local encoders: { [string]: (lib_json_Input) -> (string?, vnd_serde_json_EncodeError?) } = {
    to_string = serde_json.to_string,
    to_string_pretty = serde_json.to_string_pretty,
  }
  for name, encode in encoders do
    local text, deep = encode(nested(128))
    assert(text == nil and deep ~= nil)
    assert(deep.kind == "too_deep" and deep.max_depth == 127 and deep.op == "vnd:serde_json." .. name)
  end

  -- what is not plain data is refused as an argument
  local cyclic: { [string]: any } = {}
  cyclic.self = cyclic
  local not_data: { any } = { print, { f = print }, buffer.create(2), { data = { buffer.create(2) } }, cyclic }
  for _, bad in not_data do
    for name, encode in encoders do
      local ok, refused = pcall(function()
        encode(bad)
      end)
      assert(not ok and tostring(refused):find("vnd:serde_json." .. name .. ": argument #1 'value'", 1, true))
    end
  end
  assert(not pcall(function()
    serde_json.from_str(42 :: any) -- deliberate refusal test: a number is not text
  end))

  -- a number that is not finite is written null
  assert(serde_json.to_string { n = 0 / 0, big = math.huge } == '{"big":null,"n":null}')
end

return out
