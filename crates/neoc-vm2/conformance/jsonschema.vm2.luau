-- intentional: validate gives true, or nil and a typed error holding every violation (instance_path, schema_path, keyword, reason; it was a ValidationResult userdata with ok() and errors() strings), compile fails with a typed error (syntax with line and column, invalid_schema with the place in the schema), a lib:json handle is validated as what it holds, and a value that is not plain data is refused as an argument by both methods (validate reported a function as a violation).
local jsonschema = require("vnd:jsonschema")

local schema = assert(jsonschema.compile([[
{
    "type": "object",
    "required": ["name"],
    "properties": { "name": { "type": "string" }, "users": { "items": { "required": ["id"] } } },
    "additionalProperties": false
}
]]))

-- validate: true, or every violation
local ok, none = schema:validate({ name = "ada" })
assert(ok == true and none == nil)
local bad, err = schema:validate({ name = 1, extra = true })
assert(bad == nil and err.kind == "invalid" and err.op == "vnd:jsonschema.Schema:validate" and #err.violations == 2)
local seen = {}
for _, violation in err.violations do
	seen[violation.keyword] = violation
	assert(type(violation.reason) == "string")
end
assert(seen.type.instance_path == "/name" and seen.type.schema_path == "/properties/name/type")
assert(seen.additionalProperties.instance_path == "" and seen.additionalProperties.schema_path == "/additionalProperties")
assert(err.message:find("(and 1 more)", 1, true) and tostring(err) == "vnd:jsonschema.Schema:validate: invalid: " .. err.message)
local _, nested = schema:validate({ name = "a", users = { { id = 1 }, {} } })
assert(#nested.violations == 1 and nested.violations[1].instance_path == "/users/1")
assert(nested.violations[1].keyword == "required" and nested.message:find("/users/1: ", 1, true) == 1)

-- compile: typed errors
local _, syntax = jsonschema.compile("{\n not json")
assert(syntax.kind == "syntax" and syntax.line == 2 and syntax.op == "vnd:jsonschema.compile")
local _, invalid = jsonschema.compile('{"properties":{"name":{"type":"nonsense"}}}')
assert(invalid.kind == "invalid_schema" and invalid.path == "/properties/name/type" and type(invalid.reason) == "string")

-- a lib:json handle is what it holds
local body = assert(require("vnd:serde_json").from_str('{"name":"ada","users":[{"id":1}]}'))
assert(schema:is_valid(body) and schema:validate(body))
body:set("extra", 1)
assert(not schema:is_valid(body))

-- what is not plain data is refused as an argument
for _, value in { print, { f = print }, buffer.create(2), { data = buffer.create(2) } } do
	for _, method in { "is_valid", "validate" } do
		local raised, refused = pcall(schema[method], schema, value)
		assert(not raised and tostring(refused):find("vnd:jsonschema.Schema:" .. method .. ": argument #1 'value'", 1, true))
	end
end
assert(not pcall(jsonschema.compile, { type = "string" }))
