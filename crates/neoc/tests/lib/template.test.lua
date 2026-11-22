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

        test("a macro that calls itself without end is an error at render, not an abort", function()
            local t = template.compile("{% macro m() %}{{ self::m() }}{% endmacro m %}{{ self::m() }}")
            expect(t ~= nil).toBe(true)
            local out, err = t:render({})
            expect(out).toBeNil()
            expect(err).toContain("levels deep")
            -- and it can be rendered again
            out, err = t:render({})
            expect(out).toBeNil()
            expect(err).toContain("levels deep")
        end)

        test("an unbounded self-call is bounded wherever the syntax tree can hide it", function()
            local bodies = {
                "{% set x = self::m() %}{{ x }}",
                "{% for i in [] %}x{% else %}{{ self::m() }}{% endfor %}",
                "{% if false %}{% elif self::m() %}x{% endif %}",
                "{% if false %}x{% else %}{{ self::m() }}{% endif %}",
                "{% filter upper %}{{ self::m() }}{% endfilter %}",
                "{{ [self::m()] | length }}",
                "{{ range(end=self::m()) | length }}",
                "{{ 1 in [self::m()] }}",
                "{{ 'a' ~ 'b' }}{{ self::m() | upper }}",
            }
            for _, body in ipairs(bodies) do
                local out, err = template.render("{% macro m() %}" .. body .. "{% endmacro m %}{{ self::m() }}", {})
                expect(out).toBeNil()
                expect(err).toContain("levels deep")
            end
        end)

        test("mutual recursion is bounded", function()
            local src = "{% macro a() %}{{ self::b() }}{% endmacro a %}{% macro b() %}x{{ self::a() }}{% endmacro b %}{{ self::a() }}"
            local out, err = template.render(src, {})
            expect(out).toBeNil()
            expect(err).toContain("levels deep")
        end)

        test("infinite recursion inside a std:thread worker is a Lua error", function()
            local thread = require("std:thread")
            local w = thread.spawn({
                source = [[
                    local template = require("lib:template")
                    local out, err = template.render(
                        "{% macro f(n) %}{% for i in [1] %}{{ self::f(n=n) }}{% endfor %}{% endmacro f %}{{ self::f(n=1) }}", {})
                    return tostring(out) .. "|" .. tostring(err)
                ]],
            })
            local result = w:await()
            expect(result).toContain("nil|template.render:")
            expect(result).toContain("levels deep")
        end)

        test("a macro that terminates on its data renders", function()
            local src = "{% macro fact(n) %}{% if n <= 1 %}1{% else %}{{ n }}*{{ self::fact(n=n-1) }}{% endif %}"
                .. "{% endmacro fact %}{{ self::fact(n=10) }}"
            expect(template.render(src, {})).toBe("10*9*8*7*6*5*4*3*2*1")
        end)

        test("a tree renders with a macro that calls itself", function()
            local src = "{% macro node(t) %}<li>{{ t.name }}{% if t.children %}<ul>"
                .. "{% for c in t.children %}{{ self::node(t=c) }}{% endfor %}</ul>{% endif %}</li>{% endmacro node %}"
                .. "<ul>{{ self::node(t=tree) }}</ul>"
            local tree = { name = "a", children = { { name = "b", children = {} },
                { name = "c", children = { { name = "d", children = {} } } } } }
            expect(template.render(src, { tree = tree })).toBe(
                "<ul><li>a<ul><li>b</li><li>c<ul><li>d</li></ul></li></ul></li></ul>")
        end)

        test("a tree nine levels deep renders and ten is past the bound", function()
            local src = "{% macro node(t) %}<li>{{ t.name }}{% if t.children %}<ul>"
                .. "{% for c in t.children %}{{ self::node(t=c) }}{% endfor %}</ul>{% endif %}</li>{% endmacro node %}"
                .. "<ul>{{ self::node(t=tree) }}</ul>"
            local function tree(depth)
                local t = { name = "leaf", children = {} }
                for i = 1, depth - 1 do t = { name = "n" .. i, children = { t } } end
                return t
            end
            expect(template.render(src, { tree = tree(9) })).toContain("leaf")
            local out, err = template.render(src, { tree = tree(10) })
            expect(out).toBeNil()
            expect(err).toContain("9 levels deep")
        end)

        test("a recursion just past its bound is an error and just inside it renders", function()
            local function fact(n)
                return "{% macro fact(n) %}{% if n <= 1 %}1{% else %}{{ n }}*{{ self::fact(n=n-1) }}{% endif %}"
                    .. "{% endmacro fact %}{{ self::fact(n=" .. n .. ") }}"
            end
            expect(template.render(fact(13), {})).toContain("13*12*")
            local over, err = template.render(fact(14), {})
            expect(over).toBeNil()
            expect(err).toContain("levels deep")
        end)

        test("a macro call as an operand of arithmetic or an ordering comparison is refused", function()
            local m = "{% macro m() %}1{% endmacro m %}"
            for _, shape in ipairs({
                "{{ 1 + self::m() }}",
                "{{ self::m() * 2 }}",
                "{% if self::m() > 0 %}a{% endif %}",
                "{% if 0 <= self::m() %}a{% endif %}",
            }) do
                local out, err = template.render(m .. shape, {})
                expect(out).toBeNil()
                expect(err).toContain("cannot be an operand")
            end
            expect(template.render(m .. '{% if self::m() == "1" %}a{% endif %}', {})).toBe("a")
        end)

        test("a filtered macro call as an operand renders", function()
            local m = "{% macro m() %}1{% endmacro m %}"
            expect(template.render(m .. "{{ self::m() | int + 1 }}", {})).toBe("2")
            expect(template.render(m .. "{% if self::m() | int > 0 %}a{% endif %}", {})).toBe("a")
        end)

        test("the recursion error does not name the guard", function()
            local out, err = template.render("{% macro f() %}{{ self::f() }}{% endmacro f %}{{ self::f() }}", {})
            expect(out).toBeNil()
            expect(err).toContain("macro recursion is more than 20 levels deep")
            expect(err:find("neoc macro recursion limit", 1, true)).toBeNil()
        end)

        test("a panic inside the renderer is an error, and the engine survives it", function()
            local out, err = template.render("{{ (0 - 9223372036854775807 - 1) % -1 }}", {})
            expect(out).toBeNil()
            expect(err).toContain("template engine failed")
            expect(template.render("{{ 1 + 1 }}", {})).toBe("2")
        end)

        test("a chain of twenty thousand macros is refused quickly, not overflowed", function()
            local parts = {}
            for i = 0, 19999 do
                parts[#parts + 1] = "{% macro m" .. i .. "() %}{{ self::m" .. (i + 1) .. "() }}{% endmacro m" .. i .. " %}"
            end
            parts[#parts + 1] = "{% macro m20000() %}end{% endmacro m20000 %}{{ self::m0() }}"
            local out, err = template.render(table.concat(parts), {})
            expect(out).toBeNil()
            expect(err).toContain("too complex")
        end)

        test("macro calls nested in macro-call arguments count as nested", function()
            local function nested(callee, k)
                return ("self::" .. callee .. "(x="):rep(k) .. "1" .. (")"):rep(k)
            end
            local parts = {}
            for i = 0, 22 do
                parts[#parts + 1] = "{% macro m" .. i .. "(x) %}{{ " .. nested("m" .. (i + 1), 8)
                    .. " }}{% endmacro m" .. i .. " %}"
            end
            parts[#parts + 1] = "{% macro m23(x) %}{{ nope }}{% endmacro m23 %}{{ self::m0(x=1) }}"
            local out, err = template.render(table.concat(parts), {})
            expect(out).toBeNil()
            expect(err).toContain("too complex")
        end)

        test("a macro that calls another macro is accepted", function()
            local t = template.compile(
                "{% macro a() %}A{% endmacro a %}{% macro b() %}{{ self::a() }}B{% endmacro b %}{{ self::b() }}")
            expect(t:render({})).toBe("AB")
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

describe("template with a buffer", function()
    local refused = "a buffer cannot be serialized; convert it explicitly first"

    test("render and Template:render refuse one in the context", function()
        local ok, err = pcall(template.render, "{{ b }}", { b = buffer.create(2) })
        expect(ok).toBe(false)
        expect(tostring(err)).toContain("template.render: " .. refused)
        local t = assert(template.compile("{{ b }}"))
        ok, err = pcall(t.render, t, { b = buffer.create(2) })
        expect(ok).toBe(false)
        expect(tostring(err)).toContain("Template.render: " .. refused)
    end)

    test("render refuses one 100 tables down in the context", function()
        local t = { buffer.create(2) }
        for _ = 2, 100 do
            t = { inner = t }
        end
        local ok, err = pcall(template.render, "{{ b }}", { b = t })
        expect(ok).toBe(false)
        expect(tostring(err)).toContain("template.render: " .. refused)
    end)

    test("a filter that returns one fails the render with (nil, err)", function()
        assert(template.add_filter("as_buffer", function()
            return buffer.create(2)
        end))
        local out, err = template.render("{{ x | as_buffer }}", { x = 1 })
        expect(out).toBeNil()
        expect(err).toContain("return value: " .. refused)
    end)
end)
