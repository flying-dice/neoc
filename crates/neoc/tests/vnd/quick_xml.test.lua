local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local fs = require("std:fs")
local io = require("std:io")
local quick_xml = require("vnd:quick_xml")

fs.create_dir_all(".tmp")

describe("quick_xml", function()

    -- decode -------------------------------------------------------------------

    describe("decode", function()
        test("decodes from a reader", function()
            local reader = io.from_string("<root><name>carol</name></root>")
            local val, err = quick_xml.decode(reader)
            expect(err).toBeNil()
            expect(val.name).toBe("carol")
        end)

        test("wrong handle type returns error", function()
            local writer, _ = fs.open({ path = ".tmp/xml_decode_err.xml", mode = "w" })
            local val, err = quick_xml.decode(writer)
            expect(val).toBeNil()
            expect(err).toContain("quick_xml.decode:")
        end)
    end)

    -- decode_string ------------------------------------------------------------

    describe("decode_string", function()
        test("decodes element with text children", function()
            local val, err = quick_xml.decode_string("<root><name>alice</name><age>30</age></root>")
            expect(err).toBeNil()
            expect(val.name).toBe("alice")
            expect(val.age).toBe("30")
        end)

        test("decodes nested element", function()
            local val, err = quick_xml.decode_string("<root><user><name>bob</name></user></root>")
            expect(err).toBeNil()
            expect(val.user.name).toBe("bob")
        end)

        test("invalid xml returns error", function()
            local val, err = quick_xml.decode_string("<unclosed")
            expect(val).toBeNil()
            expect(err).toContain("quick_xml.decode_string:")
        end)
    end)

    -- encode -------------------------------------------------------------------

    describe("encode", function()
        test("encodes a table as xml", function()
            local s, err = quick_xml.encode({ name = "dave" })
            expect(err).toBeNil()
            expect(s).toContain("dave")
        end)

        test("round-trips a simple structure", function()
            local original = { name = "eve", age = "42" }
            local s, _ = quick_xml.encode(original)
            local decoded, err = quick_xml.decode_string(s)
            expect(err).toBeNil()
            expect(decoded.name).toBe("eve")
            expect(decoded.age).toBe("42")
        end)
    end)

    -- encode_to ----------------------------------------------------------------

    describe("encode_to", function()
        test("writes xml to a file writer", function()
            local writer, _ = fs.open({ path = ".tmp/xml_encode_to.xml", mode = "w" })
            local ok, err = quick_xml.encode_to({ result = "ok" }, writer)
            expect(err).toBeNil()
            expect(ok).toBe(true)
            writer:flush()

            local data, _ = fs.read(".tmp/xml_encode_to.xml")
            expect(data).toContain("ok")
        end)

        test("wrong handle type returns error", function()
            local reader = io.from_string("")
            local ok, err = quick_xml.encode_to({ x = "1" }, reader)
            expect(ok).toBeNil()
            expect(err).toContain("quick_xml.encode_to:")
        end)
    end)

    -- a buffer (refused, never turned into an array of numbers) ----------------

    describe("a buffer", function()
        local refused = "a buffer cannot be serialized; convert it explicitly first"

        test("encode refuses one", function()
            local ok, err = pcall(quick_xml.encode, { root = { data = buffer.create(2) } })
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("quick_xml.encode: " .. refused)
        end)

        test("encode_to refuses one and writes nothing", function()
            local writer, _ = fs.open({ path = ".tmp/xml_buffer.xml", mode = "w" })
            local ok, err = pcall(quick_xml.encode_to, { root = { data = buffer.create(2) } }, writer)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("quick_xml.encode_to: " .. refused)
            writer:flush()
            expect(fs.read(".tmp/xml_buffer.xml")).toBe("")
        end)
    end)
end)
