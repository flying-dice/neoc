local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local fs = require("std:fs")
local io = require("std:io")
local toml = require("vnd:toml")

fs.create_dir_all(".tmp")

-- TOML requires the root value to be a table (mapping). Scalars and arrays
-- at the root are not valid TOML and will return an encode error.

describe("toml", function()

    -- decode -------------------------------------------------------------------

    describe("decode", function()
        test("decodes table from reader", function()
            local reader = io.from_string('[server]\nhost = "localhost"\nport = 8080\n')
            local val, err = toml.decode(reader)
            expect(err).toBeNil()
            expect(val.server.host).toBe("localhost")
            expect(val.server.port).toBe(8080)
        end)

        test("decodes from a file reader", function()
            fs.write(".tmp/toml_decode_file.toml", 'name = "test"\n')
            local reader, _ = fs.open({ path = ".tmp/toml_decode_file.toml", mode = "r" })
            local val, err = toml.decode(reader)
            expect(err).toBeNil()
            expect(val.name).toBe("test")
        end)

        test("invalid toml returns error", function()
            local reader = io.from_string("key = [unclosed")
            local val, err = toml.decode(reader)
            expect(val).toBeNil()
            expect(err).toContain("toml.decode:")
        end)

        test("wrong handle type returns error", function()
            local writer, _ = fs.open({ path = ".tmp/toml_decode_err.toml", mode = "w" })
            local val, err = toml.decode(writer)
            expect(val).toBeNil()
            expect(err).toContain("toml.decode:")
        end)
    end)

    -- decode_string ------------------------------------------------------------

    describe("decode_string", function()
        test("decodes key-value pairs", function()
            local val, err = toml.decode_string('name = "alice"\nage = 30\n')
            expect(err).toBeNil()
            expect(val.name).toBe("alice")
            expect(val.age).toBe(30)
        end)

        test("decodes nested tables", function()
            local val, err = toml.decode_string('[db]\nhost = "localhost"\n')
            expect(err).toBeNil()
            expect(val.db.host).toBe("localhost")
        end)

        test("decodes inline array", function()
            local val, err = toml.decode_string("nums = [1, 2, 3]\n")
            expect(err).toBeNil()
            expect(val.nums[1]).toBe(1)
            expect(val.nums[3]).toBe(3)
        end)

        test("decodes boolean values", function()
            local val, err = toml.decode_string("enabled = true\n")
            expect(err).toBeNil()
            expect(val.enabled).toBe(true)
        end)

        test("invalid toml returns error", function()
            local val, err = toml.decode_string("bad = [unclosed")
            expect(val).toBeNil()
            expect(err).toContain("toml.decode_string:")
        end)

        test("empty string returns error", function()
            -- empty string is valid TOML (empty document = empty table)
            local val, err = toml.decode_string("")
            expect(err).toBeNil()
        end)
    end)

    -- encode -------------------------------------------------------------------

    describe("encode", function()
        test("encodes a table as toml", function()
            local s, err = toml.encode({ name = "bob", age = 25 })
            expect(err).toBeNil()
            local roundtrip, _ = toml.decode_string(s)
            expect(roundtrip.name).toBe("bob")
            expect(roundtrip.age).toBe(25)
        end)

        test("round-trips nested tables", function()
            local original = { server = { host = "localhost", port = 3000 } }
            local s, _     = toml.encode(original)
            local decoded, err = toml.decode_string(s)
            expect(err).toBeNil()
            expect(decoded.server.host).toBe("localhost")
            expect(decoded.server.port).toBe(3000)
        end)
    end)

    -- encode_pretty ------------------------------------------------------------

    describe("encode_pretty", function()
        test("output is valid toml", function()
            local s, err = toml.encode_pretty({ key = "value" })
            expect(err).toBeNil()
            local val, verr = toml.decode_string(s)
            expect(verr).toBeNil()
            expect(val.key).toBe("value")
        end)
    end)

    -- encode_to ----------------------------------------------------------------

    describe("encode_to", function()
        test("writes toml to a file writer", function()
            local writer, _ = fs.open({ path = ".tmp/toml_encode_to.toml", mode = "w" })
            local ok, err = toml.encode_to({ result = "ok" }, writer)
            expect(err).toBeNil()
            expect(ok).toBe(true)
            writer:flush()

            local data, _ = fs.read(".tmp/toml_encode_to.toml")
            local val, _ = toml.decode_string(data)
            expect(val.result).toBe("ok")
        end)

        test("wrong handle type returns error", function()
            local reader = io.from_string("")
            local ok, err = toml.encode_to({ x = 1 }, reader)
            expect(ok).toBeNil()
            expect(err).toContain("toml.encode_to:")
        end)
    end)

end)
