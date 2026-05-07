local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local jsonschema = require("vnd:jsonschema")

local USER_SCHEMA_JSON = [[
{
    "type": "object",
    "required": ["name", "username", "email"],
    "properties": {
        "name":     { "type": "string", "maxLength": 255 },
        "username": { "type": "string", "minLength": 1, "maxLength": 64 },
        "email":    { "type": "string", "maxLength": 255 },
        "phone":    { "type": "string" }
    },
    "additionalProperties": false
}
]]

describe("jsonschema", function()

    -- compile -----------------------------------------------------------------
    describe("compile", function()
        test("returns a Schema for a valid schema document", function()
            local s, err = jsonschema.compile(USER_SCHEMA_JSON)
            expect(err).toBeNil()
            expect(s).toBeTruthy()
        end)

        test("malformed schema returns error", function()
            local s, err = jsonschema.compile("not json")
            expect(s).toBeNil()
            expect(err).toContain("jsonschema.compile:")
        end)
    end)

    -- is_valid (fast path) ----------------------------------------------------
    describe("is_valid", function()
        local s = jsonschema.compile(USER_SCHEMA_JSON)

        test("accepts a valid Lua table", function()
            expect(s:is_valid({ name = "alice", username = "al", email = "a@x" })).toBe(true)
        end)

        test("rejects missing required field", function()
            expect(s:is_valid({ name = "alice" })).toBe(false)
        end)

        test("rejects wrong type", function()
            expect(s:is_valid({ name = 1, username = "al", email = "a@x" })).toBe(false)
        end)

        test("rejects extra fields under additionalProperties:false", function()
            expect(s:is_valid({ name = "a", username = "u", email = "e", extra = 1 })).toBe(false)
        end)
    end)

    -- validate (full result) --------------------------------------------------
    describe("validate", function()
        local s = jsonschema.compile(USER_SCHEMA_JSON)

        test("ok=true and empty errors when valid", function()
            local r = s:validate({ name = "a", username = "u", email = "e" })
            expect(r:ok()).toBe(true)
            expect(#r:errors()).toBe(0)
        end)

        test("ok=false and non-empty errors when invalid", function()
            local r = s:validate({ name = "a" })
            expect(r:ok()).toBe(false)
            expect(#r:errors() > 0).toBe(true)
        end)

        test("errors is a Lua table of strings", function()
            local errs = s:validate({}):errors()
            expect(type(errs[1])).toBe("string")
        end)
    end)

    -- primitive inputs --------------------------------------------------------
    describe("primitive validation", function()
        test("validates a string against type:string schema", function()
            local s = jsonschema.compile('{"type":"string"}')
            expect(s:is_valid("hi")).toBe(true)
            expect(s:is_valid(1)).toBe(false)
        end)

        test("validates an integer against type:integer schema", function()
            local s = jsonschema.compile('{"type":"integer","minimum":0}')
            expect(s:is_valid(5)).toBe(true)
            expect(s:is_valid(-1)).toBe(false)
        end)
    end)

end)
