local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local fs = require("std:fs")
local io = require("std:io")
local serde_yaml = require("vnd:serde_yaml")

fs.create_dir_all(".tmp")

describe("serde_yaml", function()

    -- decode -------------------------------------------------------------------

    describe("decode", function()
        test("decodes mapping from reader", function()
            local reader = io.from_string("name: alice\nage: 30\n")
            local val, err = serde_yaml.decode(reader)
            expect(err).toBeNil()
            expect(val.name).toBe("alice")
            expect(val.age).toBe(30)
        end)

        test("decodes sequence from reader", function()
            local reader = io.from_string("- 1\n- 2\n- 3\n")
            local val, err = serde_yaml.decode(reader)
            expect(err).toBeNil()
            expect(val[1]).toBe(1)
            expect(val[3]).toBe(3)
        end)

        test("decodes from a file reader", function()
            fs.write(".tmp/yaml_decode_file.yaml", "ok: true\n")
            local reader, _ = fs.open({ path = ".tmp/yaml_decode_file.yaml", mode = "r" })
            local val, err = serde_yaml.decode(reader)
            expect(err).toBeNil()
            expect(val.ok).toBe(true)
        end)

        test("invalid yaml returns error", function()
            local reader = io.from_string("key: [unclosed")
            local val, err = serde_yaml.decode(reader)
            expect(val).toBeNil()
            expect(err).toContain("serde_yaml.decode:")
        end)

        test("wrong handle type returns error", function()
            local writer, _ = fs.open({ path = ".tmp/yaml_decode_err.yaml", mode = "w" })
            local val, err = serde_yaml.decode(writer)
            expect(val).toBeNil()
            expect(err).toContain("serde_yaml.decode:")
        end)
    end)

    -- decode_string ------------------------------------------------------------

    describe("decode_string", function()
        test("decodes mapping", function()
            local val, err = serde_yaml.decode_string("x: 1\ny: 2\n")
            expect(err).toBeNil()
            expect(val.x).toBe(1)
            expect(val.y).toBe(2)
        end)

        test("decodes sequence", function()
            local val, err = serde_yaml.decode_string("- a\n- b\n")
            expect(err).toBeNil()
            expect(val[1]).toBe("a")
            expect(val[2]).toBe("b")
        end)

        test("decodes string scalar", function()
            local val, err = serde_yaml.decode_string("hello\n")
            expect(err).toBeNil()
            expect(val).toBe("hello")
        end)

        test("decodes boolean", function()
            local val, err = serde_yaml.decode_string("true\n")
            expect(err).toBeNil()
            expect(val).toBe(true)
        end)

        test("decodes null field as the JSON-null sentinel (not Lua nil)", function()
            -- See the matching note in serde_json.test.luau: null lands as
            -- a sentinel userdata, not nil, because decode goes through
            -- `mlua::LuaSerdeExt::to_value`.
            local val, err = serde_yaml.decode_string("key: null\n")
            expect(err).toBeNil()
            expect(val.key).toBeTruthy()
        end)

        test("invalid yaml returns error", function()
            local val, err = serde_yaml.decode_string("key: [unclosed")
            expect(val).toBeNil()
            expect(err).toContain("serde_yaml.decode_string:")
        end)
    end)

    -- encode -------------------------------------------------------------------

    describe("encode", function()
        test("encodes a table as yaml mapping", function()
            local s, err = serde_yaml.encode({ name = "bob" })
            expect(err).toBeNil()
            local roundtrip, _ = serde_yaml.decode_string(s)
            expect(roundtrip.name).toBe("bob")
        end)

        test("encodes a number", function()
            local s, err = serde_yaml.encode(42)
            expect(err).toBeNil()
            local roundtrip, _ = serde_yaml.decode_string(s)
            expect(roundtrip).toBe(42)
        end)

        test("encodes a boolean", function()
            local s, err = serde_yaml.encode(true)
            expect(err).toBeNil()
            local roundtrip, _ = serde_yaml.decode_string(s)
            expect(roundtrip).toBe(true)
        end)

        test("round-trips a nested structure", function()
            local original = { user = { name = "carol", active = true } }
            local s, _     = serde_yaml.encode(original)
            local decoded, err = serde_yaml.decode_string(s)
            expect(err).toBeNil()
            expect(decoded.user.name).toBe("carol")
            expect(decoded.user.active).toBe(true)
        end)
    end)

    -- encode_to ----------------------------------------------------------------

    describe("encode_to", function()
        test("writes yaml to a file writer", function()
            local writer, _ = fs.open({ path = ".tmp/yaml_encode_to.yaml", mode = "w" })
            local ok, err = serde_yaml.encode_to({ result = "ok" }, writer)
            expect(err).toBeNil()
            expect(ok).toBe(true)
            writer:flush()

            local data, _ = fs.read(".tmp/yaml_encode_to.yaml")
            local val, _ = serde_yaml.decode_string(data)
            expect(val.result).toBe("ok")
        end)

        test("wrong handle type returns error", function()
            local reader = io.from_string("")
            local ok, err = serde_yaml.encode_to({ x = 1 }, reader)
            expect(ok).toBeNil()
            expect(err).toContain("serde_yaml.encode_to:")
        end)
    end)

end)
