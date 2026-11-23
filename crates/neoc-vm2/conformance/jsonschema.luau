-- vnd:jsonschema on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- What validate gives, the errors and what is refused differ on purpose: see jsonschema.vm2.luau.
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

local user, err = jsonschema.compile(USER_SCHEMA)
out.compiles = { user ~= nil, err == nil }
out.user = {
	user:is_valid({ name = "alice", username = "al", email = "a@x" }),
	user:is_valid({ name = "alice", username = "al", email = "a@x", tags = { "a", "b" }, address = { city = "x", zip = "12345" } }),
	user:is_valid({ name = "alice" }),
	user:is_valid({ name = 1, username = "al", email = "a@x" }),
	user:is_valid({ name = "a", username = "", email = "e" }),
	user:is_valid({ name = "a", username = "u", email = "e", extra = 1 }),
	user:is_valid({ name = "a", username = "u", email = "e", tags = { "a", "a" } }),
	user:is_valid({ name = "a", username = "u", email = "e", tags = { "a", 1 } }),
	user:is_valid({ name = "a", username = "u", email = "e", address = { zip = "12345" } }),
	user:is_valid({ name = "a", username = "u", email = "e", address = { city = "x", zip = "1234" } }),
	user:is_valid("text"),
	user:is_valid(42),
	user:is_valid(nil),
}

local function check(schema, values)
	local compiled = assert(jsonschema.compile(schema))
	local answers = {}
	for i, value in values do
		answers[i] = compiled:is_valid(value)
	end
	return answers
end
out.string = check('{"type":"string","minLength":2}', { "hi", "h", 1, true })
out.integer = check('{"type":"integer","minimum":0,"maximum":10}', { 5, 0, 10, -1, 11, 1.5, "5" })
out.number = check('{"type":"number","exclusiveMinimum":0,"multipleOf":0.5}', { 0.5, 2, 0, 0.3 })
out.enum = check('{"enum":["red","green",3]}', { "red", 3, "blue", 4 })
out.any_of = check('{"anyOf":[{"type":"string"},{"type":"number"}]}', { "x", 1, true })
out.const_and_not = check('{"not":{"const":"no"}}', { "yes", "no" })
out.draft_07 = check('{"$schema":"http://json-schema.org/draft-07/schema#","type":"array","items":[{"type":"string"},{"type":"number"}]}', {
	{ "a", 1 },
	{ 1, "a" },
})
out.definitions = check('{"$defs":{"id":{"type":"integer","minimum":1}},"type":"object","properties":{"id":{"$ref":"#/$defs/id"}}}', {
	{ id = 3 },
	{ id = 0 },
	{ id = "3" },
})
out.idn_hostname = check('{"$schema":"http://json-schema.org/draft-07/schema#","type":"string","format":"idn-hostname"}', {
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

-- a failure is `(nil, err)`; what `err` is differs
for name, text in {
	not_json = "not json",
	empty = "",
	bad_type = '{"type":"nonsense"}',
	bad_pattern = '{"pattern":"("}',
	remote_ref = '{"$ref":"https://example.invalid/schema.json"}',
} do
	local schema, why = jsonschema.compile(text)
	out["fails_" .. name] = { schema == nil, why ~= nil }
end

-- a value with no JSON form makes is_valid raise
out.raises = { (pcall(user.is_valid, user, print)), (pcall(user.is_valid, user, { data = buffer.create(2) })) }

return out
