local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local fs = require("std:fs")
local io = require("std:io")
local serde_json = require("vnd:serde_json")

fs.create_dir_all(".tmp")

describe("serde_json", function()

    -- from_reader --------------------------------------------------------------

    describe("from_reader", function()
        test("decodes object from reader", function()
            local reader = io.from_string('{"name":"alice","age":30}')
            local val, err = serde_json.from_reader(reader)
            expect(err).toBeNil()
            expect(val:get("name")).toBe("alice")
            expect(val:get("age")).toBe(30)
        end)

        test("decodes array from reader", function()
            local reader = io.from_string('[1,2,3]')
            local val, err = serde_json.from_reader(reader)
            expect(err).toBeNil()
            expect(val:get(1)).toBe(1)
            expect(val:get(3)).toBe(3)
        end)

        test("decodes from a file reader", function()
            fs.write(".tmp/json_decode_file.json", '{"ok":true}')
            local reader, _ = fs.open({ path = ".tmp/json_decode_file.json", mode = "r" })
            local val, err = serde_json.from_reader(reader)
            expect(err).toBeNil()
            expect(val:get("ok")).toBe(true)
        end)

        test("invalid json returns error", function()
            local reader = io.from_string("not json")
            local val, err = serde_json.from_reader(reader)
            expect(val).toBeNil()
            expect(err).toContain("serde_json.from_reader:")
        end)

        test("wrong handle type returns error", function()
            local writer, _ = fs.open({ path = ".tmp/json_decode_err.json", mode = "w" })
            local val, err = serde_json.from_reader(writer)
            expect(val).toBeNil()
            expect(err).toContain("serde_json.from_reader:")
        end)
    end)

    -- from_str -----------------------------------------------------------------

    describe("from_str", function()
        test("decodes object", function()
            local val, err = serde_json.from_str('{"x":1}')
            expect(err).toBeNil()
            expect(val:get("x")).toBe(1)
        end)

        test("decodes array", function()
            local val, err = serde_json.from_str('["a","b"]')
            expect(err).toBeNil()
            expect(val:get(1)).toBe("a")
            expect(val:get(2)).toBe("b")
        end)

        test("decodes string scalar", function()
            local val, err = serde_json.from_str('"hello"')
            expect(err).toBeNil()
            expect(val).toBe("hello")
        end)

        test("decodes number scalar", function()
            local val, err = serde_json.from_str("42")
            expect(err).toBeNil()
            expect(val).toBe(42)
        end)

        test("decodes boolean", function()
            local val, err = serde_json.from_str("true")
            expect(err).toBeNil()
            expect(val).toBe(true)
        end)

        test("decodes null as the lib:json null sentinel (not Lua nil)", function()
            -- Decode goes through `lib:json::wrap_root`, which maps JSON
            -- `null` to the sentinel userdata so scripts can distinguish
            -- "key absent" from "explicit null". Compare against
            -- `(require("lib:json")).null`.
            local json = require("lib:json")
            local val, err = serde_json.from_str("null")
            expect(err).toBeNil()
            expect(val).toBe(json.null)
        end)

        test("invalid json returns error", function()
            local val, err = serde_json.from_str("{bad}")
            expect(val).toBeNil()
            expect(err).toContain("serde_json.from_str:")
        end)

        test("empty string returns error", function()
            local val, err = serde_json.from_str("")
            expect(val).toBeNil()
            expect(err).toContain("serde_json.from_str:")
        end)
    end)

    -- to_string ----------------------------------------------------------------

    describe("to_string", function()
        test("encodes a table as a json object", function()
            local s, err = serde_json.to_string({ key = "value" })
            expect(err).toBeNil()
            local roundtrip, _ = serde_json.from_str(s)
            expect(roundtrip:get("key")).toBe("value")
        end)

        test("encodes a number", function()
            local s, err = serde_json.to_string(42)
            expect(err).toBeNil()
            expect(s).toBe("42")
        end)

        test("encodes a string", function()
            local s, err = serde_json.to_string("hello")
            expect(err).toBeNil()
            expect(s).toBe('"hello"')
        end)

        test("encodes a boolean", function()
            local s, err = serde_json.to_string(true)
            expect(err).toBeNil()
            expect(s).toBe("true")
        end)

        test("encodes nil as null", function()
            local s, err = serde_json.to_string(nil)
            expect(err).toBeNil()
            expect(s).toBe("null")
        end)

        test("round-trips a nested structure", function()
            local original = { name = "bob", scores = { 1, 2, 3 }, active = true }
            local s, _     = serde_json.to_string(original)
            local decoded, _ = serde_json.from_str(s)
            expect(decoded:get("name")).toBe("bob")
            expect(decoded:get("active")).toBe(true)
        end)
    end)

    -- to_string_pretty ---------------------------------------------------------

    describe("to_string_pretty", function()
        test("output contains newlines", function()
            local s, err = serde_json.to_string_pretty({ a = 1 })
            expect(err).toBeNil()
            expect(s).toContain("\n")
        end)

        test("pretty output is valid json", function()
            local s, _ = serde_json.to_string_pretty({ x = 99 })
            local val, err = serde_json.from_str(s)
            expect(err).toBeNil()
            expect(val:get("x")).toBe(99)
        end)
    end)

    -- to_writer ----------------------------------------------------------------

    describe("to_writer", function()
        test("writes json to a file writer", function()
            local writer, _ = fs.open({ path = ".tmp/json_encode_to.json", mode = "w" })
            local ok, err = serde_json.to_writer({ result = "ok" }, writer)
            expect(err).toBeNil()
            expect(ok).toBe(true)
            writer:flush()

            local data, _ = fs.read(".tmp/json_encode_to.json")
            local val, _ = serde_json.from_str(data)
            expect(val:get("result")).toBe("ok")
        end)

        test("wrong handle type returns error", function()
            local reader = io.from_string("")
            local ok, err = serde_json.to_writer({ x = 1 }, reader)
            expect(ok).toBeNil()
            expect(err).toContain("serde_json.to_writer:")
        end)
    end)


    -- nesting limit ------------------------------------------------------------

    describe("nesting limit", function()
        local function nested(depth)
            local v = {}
            for _ = 2, depth do v = { v } end
            return v
        end

        -- an error is raised or returned as (nil, err), whichever the function does
        local function failure(f, ...)
            local ok, a, b = pcall(f, ...)
            if not ok then return tostring(a) end
            if a == nil then return tostring(b) end
            return nil
        end

        test("127 levels are written and read back, 128 are refused by every writer", function()
            local text = serde_json.to_string(nested(127))
            expect(text).toBeTruthy()
            local back, err = serde_json.from_str(text)
            expect(err).toBeNil()
            expect(back).toBeTruthy()

            for _, f in ipairs({ serde_json.to_string, serde_json.to_string_pretty }) do
                expect(failure(f, nested(128))).toContain("levels deep")
            end
            local writer = fs.open({ path = ".tmp/json_too_deep.json", mode = "w" })
            expect(failure(serde_json.to_writer, nested(128), writer)).toContain("levels deep")
        end)

        test("a table that contains itself is an error", function()
            local t = {}
            t.self = t
            expect(failure(serde_json.to_string, t)).toContain("levels deep")
        end)
    end)

    -- a buffer (refused, never turned into an array of numbers) ----------------

    describe("a buffer", function()
        local refused = "a buffer cannot be serialized; convert it explicitly first"

        test("to_string and to_string_pretty refuse one anywhere in the value", function()
            local ok, err = pcall(serde_json.to_string, { data = { buffer.fromstring("ab") } })
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("serde_json.to_string: " .. refused)
            ok, err = pcall(serde_json.to_string_pretty, buffer.create(2))
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("serde_json.to_string_pretty: " .. refused)
        end)

        test("to_writer refuses one and writes nothing", function()
            local writer, _ = fs.open({ path = ".tmp/json_buffer.json", mode = "w" })
            local ok, err = pcall(serde_json.to_writer, { data = buffer.create(2) }, writer)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("serde_json.to_writer: " .. refused)
            writer:flush()
            expect(fs.read(".tmp/json_buffer.json")).toBe("")
        end)

        test("the text of a buffer encodes as a string", function()
            local out, err = serde_json.to_string({ data = buffer.tostring(buffer.fromstring("ab")) })
            expect(err).toBeNil()
            expect(out).toBe('{"data":"ab"}')
        end)
    end)
end)
