local _t = require("lib:test")
local describe = _t.describe
local test = _t.test
local expect = _t.expect

local ini = require("lib:ini")

describe("ini.parse", function()
    test("basic section with key-value", function()
        local result, err = ini.parse("[section]\nkey=value")
        expect(err).toBeNil()
        expect(result.section.key).toBe("value")
    end)

    test("sectionless keys go under empty string key", function()
        local result = ini.parse("name=world")
        expect(result[""].name).toBe("world")
    end)

    test("empty value", function()
        local result = ini.parse("[s]\nkey=")
        expect(result.s.key).toBe("")
    end)

    test("bare key maps to true", function()
        local result = ini.parse("[s]\nflag")
        expect(result.s.flag).toBe(true)
    end)

    test("duplicate keys — last wins", function()
        local result = ini.parse("[s]\nk=a\nk=b")
        expect(result.s.k).toBe("b")
    end)

    test("duplicate sections merge", function()
        local result = ini.parse("[s]\na=1\n[s]\nb=2")
        expect(result.s.a).toBe("1")
        expect(result.s.b).toBe("2")
    end)

    test("empty input", function()
        local result = ini.parse("")
        local count = 0
        for _ in result do
            count += 1
        end
        expect(count).toBe(0)
    end)

    test("comments only", function()
        local result = ini.parse("; comment\n# another")
        local count = 0
        for _ in result do
            count += 1
        end
        expect(count).toBe(0)
    end)

    test("no inline comments", function()
        local result = ini.parse("[s]\nkey=value ; not a comment")
        expect(result.s.key).toBe("value ; not a comment")
    end)

    test("section with no keys", function()
        local result = ini.parse("[empty]\n[other]\nk=v")
        expect(result.other.k).toBe("v")
    end)

    test("unicode keys and values", function()
        local result = ini.parse("[日本語]\nキー=値")
        expect(result["日本語"]["キー"]).toBe("値")
    end)

    test("lowercase_keys option", function()
        local result = ini.parse("[Section]\nKey=val", { lowercase_keys = true })
        expect(result.section.key).toBe("val")
    end)

    test("custom comment symbols", function()
        local result = ini.parse("!ignored\nkey=val", { comment_symbols = { "!" } })
        expect(result[""].key).toBe("val")
    end)

    test("custom delimiters", function()
        local result = ini.parse("[s]\nkey:val", { delimiters = { ":" } })
        expect(result.s.key).toBe("val")
    end)

    test("error: unclosed section header", function()
        local val, err = ini.parse("[bad")
        expect(val).toBeNil()
        expect(err).toContain("unclosed section")
    end)

    test("error: empty section name", function()
        local val, err = ini.parse("[]")
        expect(val).toBeNil()
        expect(err).toContain("empty section name")
    end)

    test("error: control bytes", function()
        local val, err = ini.parse("key=val\0ue")
        expect(val).toBeNil()
        expect(err).toContain("control bytes")
    end)
end)

describe("ini.stringify", function()
    test("basic section", function()
        local t = { section = { key = "value" } }
        local s, err = ini.stringify(t)
        expect(err).toBeNil()
        expect(s).toContain("[section]")
        expect(s).toContain("key=value")
    end)

    test("bare key (true) written without delimiter", function()
        local t = { section = { flag = true } }
        local s = ini.stringify(t)
        expect(s).toContain("flag")
    end)

    test("sectionless keys written first", function()
        local t = { [""] = { name = "world" }, section = { k = "v" } }
        local s = ini.stringify(t)
        expect(s).toContain("name=world")
    end)

    test("spacing=true adds spaces around delimiter", function()
        local t = { section = { key = "value" } }
        local s = ini.stringify(t, { spacing = true })
        expect(s).toContain("key = value")
    end)

    test("spacing=false uses compact delimiter", function()
        local t = { section = { key = "value" } }
        local s = ini.stringify(t, { spacing = false })
        expect(s).toContain("key=value")
    end)

    test("round trip preserves data", function()
        local input = "[db]\nhost=localhost\nport=5432"
        local parsed = ini.parse(input)
        local output = ini.stringify(parsed)
        expect(output).toContain("host=localhost")
        expect(output).toContain("port=5432")
    end)
end)
