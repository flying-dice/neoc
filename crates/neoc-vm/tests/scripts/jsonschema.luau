--!strict
-- vnd:jsonschema as a script sees it: its answers (jsonschema.json), then what it asserts.
local jsonschema = require("vnd:jsonschema")
local out = {}

local USER_SCHEMA = [[
{
    "type": "object",
    "required": ["name", "username", "email"],
    "properties": {
        "name":     { "type": "string", "maxLength": 255 },
        "username": { "type": "string", "minLength": 1, "maxLength": 64 },
        "email":    { "type": "string", "maxLength": 255 },
        "phone":    { "type": "string" },
        "tags":     { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
        "address":  { "type": "object", "required": ["city"], "properties": { "zip": { "pattern": "^[0-9]{5}$" } } }
    },
    "additionalProperties": false
}
]]

local compiled_user, err = jsonschema.compile(USER_SCHEMA)
out.compiles = { (compiled_user :: any) ~= nil, err == nil }
local user = assert(compiled_user)
out.user = {
  user:is_valid { name = "alice", username = "al", email = "a@x" },
  user:is_valid {
    name = "alice",
    username = "al",
    email = "a@x",
    tags = { "a", "b" },
    address = { city = "x", zip = "12345" },
  },
  user:is_valid { name = "alice" },
  user:is_valid { name = 1, username = "al", email = "a@x" },
  user:is_valid { name = "a", username = "", email = "e" },
  user:is_valid { name = "a", username = "u", email = "e", extra = 1 },
  user:is_valid { name = "a", username = "u", email = "e", tags = { "a", "a" } },
  user:is_valid { name = "a", username = "u", email = "e", tags = { "a", 1 } :: { any } },
  user:is_valid { name = "a", username = "u", email = "e", address = { zip = "12345" } },
  user:is_valid { name = "a", username = "u", email = "e", address = { city = "x", zip = "1234" } },
  user:is_valid("text"),
  user:is_valid(42),
  user:is_valid(nil),
}

local function check(schema: string, values: { any }): { boolean }
  local compiled = assert(jsonschema.compile(schema))
  local answers = {}
  for i, value in values do
    answers[i] = compiled:is_valid(value)
  end
  return answers
end
out.string = check('{"type":"string","minLength":2}', { "hi", "h", 1, true } :: { any })
out.integer = check('{"type":"integer","minimum":0,"maximum":10}', { 5, 0, 10, -1, 11, 1.5, "5" } :: { any })
out.number = check('{"type":"number","exclusiveMinimum":0,"multipleOf":0.5}', { 0.5, 2, 0, 0.3 })
out.enum = check('{"enum":["red","green",3]}', { "red", 3, "blue", 4 } :: { any })
out.any_of = check('{"anyOf":[{"type":"string"},{"type":"number"}]}', { "x", 1, true } :: { any })
out.const_and_not = check('{"not":{"const":"no"}}', { "yes", "no" })
out.draft_07 = check(
  '{"$schema":"http://json-schema.org/draft-07/schema#","type":"array","items":[{"type":"string"},{"type":"number"}]}',
  {
    { "a", 1 } :: { any },
    { 1, "a" } :: { any },
  }
)
out.definitions =
  check('{"$defs":{"id":{"type":"integer","minimum":1}},"type":"object","properties":{"id":{"$ref":"#/$defs/id"}}}', {
    { id = 3 },
    { id = 0 },
    { id = "3" },
  })
out.idn_hostname =
  check('{"$schema":"http://json-schema.org/draft-07/schema#","type":"string","format":"idn-hostname"}', {
    "example.com",
    "\195\188ber.example",
    "-bad-",
    "a..b",
    "xn--",
    string.rep("a", 64) .. ".com",
  })
out.idn_email = check('{"$schema":"http://json-schema.org/draft-07/schema#","type":"string","format":"idn-email"}', {
  "user@example.com",
  "\206\177@\207\128\206\177\207\129.gr",
  "not an email",
  "@nope",
})

-- a failure is `(nil, err)`
for name, text in
  {
    not_json = "not json",
    empty = "",
    bad_type = '{"type":"nonsense"}',
    bad_pattern = '{"pattern":"("}',
    remote_ref = '{"$ref":"https://example.invalid/schema.json"}',
  } :: { [string]: string }
do
  local schema, why = jsonschema.compile(text)
  out["fails_" .. name] = { schema == nil, why ~= nil }
end

-- a value with no JSON form makes is_valid raise
out.raises =
  { (pcall(user.is_valid :: any, user, print)), (pcall(user.is_valid :: any, user, { data = buffer.create(2) })) }

-- what it asserts
do
  local schema = assert(jsonschema.compile([[
  {
      "type": "object",
      "required": ["name"],
      "properties": { "name": { "type": "string" }, "users": { "items": { "required": ["id"] } } },
      "additionalProperties": false
  }
  ]]))

  -- validate: true, or every violation
  local ok, none = schema:validate { name = "ada" }
  assert(ok == true and none == nil)
  local bad, err = schema:validate { name = 1, extra = true }
  assert(bad == nil and err ~= nil)
  assert(err.kind == "invalid" and err.op == "vnd:jsonschema.Schema:validate" and #err.violations == 2)
  local seen = {}
  for _, violation in err.violations do
    seen[violation.keyword] = violation
    assert(type(violation.reason) == "string")
  end
  assert(seen.type.instance_path == "/name" and seen.type.schema_path == "/properties/name/type")
  assert(
    seen.additionalProperties.instance_path == "" and seen.additionalProperties.schema_path == "/additionalProperties"
  )
  assert(
    err.message:find("(and 1 more)", 1, true)
      and tostring(err) == "vnd:jsonschema.Schema:validate: invalid: " .. err.message
  )
  local users: { any } = { { id = 1 }, {} }
  local _, nested = schema:validate { name = "a", users = users }
  assert(nested ~= nil)
  assert(#nested.violations == 1 and nested.violations[1].instance_path == "/users/1")
  assert(nested.violations[1].keyword == "required" and nested.message:find("/users/1: ", 1, true) == 1)

  -- compile: typed errors
  local _, syntax = jsonschema.compile("{\n not json")
  assert(syntax ~= nil and syntax.kind == "syntax" and syntax.line == 2 and syntax.op == "vnd:jsonschema.compile")
  local _, invalid = jsonschema.compile('{"properties":{"name":{"type":"nonsense"}}}')
  assert(
    invalid ~= nil
      and invalid.kind == "invalid_schema"
      and invalid.path == "/properties/name/type"
      and type(invalid.reason) == "string"
  )

  -- a lib:json handle is what it holds
  local body = assert(require("vnd:serde_json").from_str('{"name":"ada","users":[{"id":1}]}'))
  assert(typeof(body) == "lib_json_Object")
  assert(schema:is_valid(body) and schema:validate(body))
  body:set("extra", 1)
  assert(not schema:is_valid(body))

  -- what is not plain data is refused as an argument
  local not_data: { any } = { print, { f = print }, buffer.create(2), { data = buffer.create(2) } }
  for _, value in not_data do
    local methods: { [string]: () -> ...unknown } = {
      is_valid = function()
        return schema:is_valid(value)
      end,
      validate = function()
        return schema:validate(value)
      end,
    }
    for method, call in methods do
      local raised, refused = pcall(call)
      assert(
        not raised and tostring(refused):find("vnd:jsonschema.Schema:" .. method .. ": argument #1 'value'", 1, true)
      )
    end
  end
  assert(not pcall(function()
    jsonschema.compile({ type = "string" } :: any) -- deliberate refusal test: a schema is text
  end))
end

return out
