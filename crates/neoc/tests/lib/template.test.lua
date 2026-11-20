local _t = require("lib:test")
local describe = _t.describe
local test = _t.test
local expect = _t.expect

local template = require("lib:template")

describe("template.render", function()
    test("substitutes variables", function()
        local out, err = template.render("Hello, {{ name }}!", { name = "world" })
        expect(err).toBeNil()
        expect(out).toBe("Hello, world!")
    end)

    test("renders loops", function()
        local out = template.render(
            "{% for n in nums %}{{ n }},{% endfor %}",
            { nums = { 1, 2, 3 } }
        )
        expect(out).toBe("1,2,3,")
    end)

    test("renders conditionals", function()
        local yes = template.render("{% if flag %}YES{% else %}NO{% endif %}", { flag = true })
        local no = template.render("{% if flag %}YES{% else %}NO{% endif %}", { flag = false })
        expect(yes).toBe("YES")
        expect(no).toBe("NO")
    end)

    test("built-in filters work", function()
        expect(template.render("{{ s | upper }}", { s = "shout" })).toBe("SHOUT")
        expect(template.render("{{ s | lower }}", { s = "QUIET" })).toBe("quiet")
        expect(template.render("{{ s | length }}", { s = "abcd" })).toBe("4")
    end)

    test("autoescape off by default", function()
        expect(template.render("{{ s }}", { s = "<b>" })).toBe("<b>")
    end)

    test("autoescape opt-in escapes HTML", function()
        local out = template.render("{{ s }}", { s = "<b>" }, { autoescape = true })
        expect(out).toBe("&lt;b&gt;")
    end)

    test("whitespace control preserved", function()
        local out = template.render("A{%- if x %}B{%- endif %}", { x = true })
        expect(out).toBe("AB")
    end)

    test("missing variable produces error", function()
        local out, err = template.render("{{ missing }}", { other = 1 })
        expect(out).toBeNil()
        expect(err).toContain("template.render:")
        expect(err).toContain("missing")
    end)

    test("malformed template error has line info", function()
        local out, err = template.render("{% for x in xs %}", { xs = {} })
        expect(out).toBeNil()
        expect(err).toContain("template.render:")
    end)

    test("context must be a table", function()
        local out, err = template.render("{{ x }}", 42)
        expect(out).toBeNil()
        expect(err).toContain("context")
    end)

    test("autoescape option must be boolean", function()
        local _, err = template.render("x", {}, { autoescape = "yes" })
        expect(err).toContain("autoescape")
        expect(err).toContain("boolean")
    end)
end)

describe("template.compile + Template:render", function()
    test("compile + render", function()
        local t = assert(template.compile("Hi {{ who }}"))
        expect(t:render({ who = "Skywarp" })).toBe("Hi Skywarp")
    end)

    test("compile error returns (nil, error)", function()
        local val, err = template.compile("{% if %}")
        expect(val).toBeNil()
        expect(err).toContain("template.compile:")
    end)

    test("compiled template renders multiple times", function()
        local t = assert(template.compile("v={{ v }}"))
        expect(t:render({ v = 1 })).toBe("v=1")
        expect(t:render({ v = 2 })).toBe("v=2")
        expect(t:render({ v = 3 })).toBe("v=3")
    end)

    test("tostring shows template name", function()
        local t = assert(template.compile("x"))
        local s = tostring(t)
        expect(s:sub(1, 9)).toBe("Template(")
    end)
end)

describe("template.add_filter", function()
    test("registered filter is visible", function()
        template.add_filter("shout_a", function(value, _args)
            return string.upper(tostring(value))
        end)
        expect(template.render("{{ s | shout_a }}", { s = "loud" })).toBe("LOUD")
    end)

    test("filter visible to template compiled BEFORE registration", function()
        local t = assert(template.compile("{{ s | shout_b }}"))
        -- Before registration: error
        local val, err = t:render({ s = "x" })
        expect(val).toBeNil()
        expect(err).toContain("shout_b")
        -- Register: now visible
        template.add_filter("shout_b", function(value, _args)
            return string.upper(tostring(value))
        end)
        expect(t:render({ s = "x" })).toBe("X")
    end)

    test("custom filter error propagates", function()
        template.add_filter("kaboom", function(_, _)
            error("filter exploded")
        end)
        local out, err = template.render("{{ x | kaboom }}", { x = 1 })
        expect(out).toBeNil()
        expect(err).toContain("template.render:")
        expect(err).toContain("kaboom")
        expect(err).toContain("filter exploded")
    end)

    test("custom filter output escaped under autoescape", function()
        template.add_filter("wrap_b", function(value, _args)
            return "<b>" .. tostring(value) .. "</b>"
        end)
        -- `/` escapes to `&#x2F;` under tera's HTML-safe variant.
        local out = template.render("{{ s | wrap_b }}", { s = "x" }, { autoescape = true })
        expect(out).toContain("&lt;b&gt;")
        expect(out).toContain("x")
        expect(out).toContain("b&gt;")
    end)

    test("custom filter args passed as table", function()
        template.add_filter("with_args", function(value, args)
            return tostring(value) .. ":" .. tostring(args.suffix)
        end)
        local out = template.render(
            '{{ s | with_args(suffix="end") }}',
            { s = "core" }
        )
        expect(out).toBe("core:end")
    end)

    test("empty filter name rejected", function()
        local _, err = template.add_filter("", function() return "" end)
        expect(err).toContain("non-empty")
    end)

    test("filter calling render is rejected, not deadlocked", function()
        template.add_filter("recurse_render", function(_, _)
            local out, e = template.render("inner:{{ s }}", { s = "x" })
            if not out then error(e) end
            return out
        end)
        local out, err = template.render("{{ x | recurse_render }}", { x = "y" })
        expect(out).toBeNil()
        expect(err).toContain("recursive render")
        expect(err).toContain("recurse_render")
    end)

    describe("hostile templates", function()
        test("include, import and extends are refused", function()
            for _, src in ipairs({
                '{% include "other" %}',
                '{% import "other" as o %}',
                '{% extends "base" %}',
            }) do
                local out, err = template.compile(src)
                expect(out).toBeNil()
                expect(err).toContain("template.compile:")
            end
        end)

        test("a template that names another is refused with the reason", function()
            local out, err = template.compile('{% include "other" %}')
            expect(out).toBeNil()
            expect(err).toContain("not supported")
        end)

        test("a refused template does not break the ones compiled after it", function()
            template.compile('{% extends "base" %}')
            template.compile('{% import "other" as o %}')
            local out = template.render("{{ x }}", { x = "still works" })
            expect(out).toBe("still works")
            local t = template.compile("{{ x }}")
            expect(t:render({ x = "compiled" })).toBe("compiled")
        end)

        test("a macro that calls itself is refused", function()
            local out, err = template.compile("{% macro m() %}{{ self::m() }}{% endmacro m %}")
            expect(out).toBeNil()
            expect(err).toContain("template.compile:")
        end)

        test("deeply nested blocks are refused", function()
            local src = ("{% if true %}"):rep(100) .. ("{% endif %}"):rep(100)
            local out, err = template.compile(src)
            expect(out).toBeNil()
            expect(err).toContain("too complex")
        end)

        test("range with a zero step or an enormous length is an error", function()
            local out, err = template.render("{% for i in range(end=5, step_by=0) %}{{ i }}{% endfor %}", {})
            expect(out).toBeNil()
            expect(err).toContain("step_by")
            out, err = template.render("{% for i in range(end=1000000000000) %}{% endfor %}", {})
            expect(out).toBeNil()
            expect(err).toContain("range")
        end)

        test("a refused one-off render does not break the next render", function()
            local out, err = template.render('{% extends "base" %}', {})
            expect(out).toBeNil()
            expect(err).toContain("template.render:")
            out = template.render("{{ x }}", { x = "ok" })
            expect(out).toBe("ok")
        end)

        test("limits are exact: 48 operators pass, 49 are refused", function()
            local function sum(operators)
                return "{{ 1" .. (" + 1"):rep(operators) .. " }}"
            end
            expect(template.render(sum(48), {})).toBe("49")
            local out, err = template.render(sum(49), {})
            expect(out).toBeNil()
            expect(err).toContain("too complex")
        end)

        test("comparisons, signs and keyword arguments are not over-counted", function()
            local alternatives = {}
            for i = 1, 20 do alternatives[#alternatives + 1] = "x == " .. i end
            local cond = table.concat(alternatives, " or ")
            expect(template.render("{% if " .. cond .. " %}hit{% endif %}", { x = 20 })).toBe("hit")
            local negatives = {}
            for i = 1, 60 do negatives[#negatives + 1] = "-" .. i end
            expect(template.render("{% set l = [" .. table.concat(negatives, ", ") .. "] %}{{ l | length }}", {}))
                .toBe("60")
            expect(template.render("{% if x >= 1 and x <= 3 and x != 2 %}in{% endif %}", { x = 3 })).toBe("in")
        end)

        test("limits are exact: 24 nested blocks pass, 25 are refused", function()
            local function nest(n)
                return ("{% if true %}"):rep(n) .. "x" .. ("{% endif %}"):rep(n)
            end
            expect(template.render(nest(24), {})).toBe("x")
            local out, err = template.render(nest(25), {})
            expect(out).toBeNil()
            expect(err).toContain("too complex")
        end)

        test("range still works for ordinary bounds", function()
            local out = template.render("{% for i in range(start=1, end=4) %}{{ i }}{% endfor %}", {})
            expect(out).toBe("123")
        end)
    end)
end)
