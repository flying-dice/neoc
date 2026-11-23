-- intentional: errors are typed (compile: syntax, too_complex, unsupported; render: those and failed, recursion_limit, recursive_render, engine_panic; add_filter: empty_name) and a too_complex reason no longer starts with "template too complex: "; a context that is not a table of names to plain data (a number, a list, a buffer inside) and a wrong or unknown option are refused as arguments with their place (they were returned errors); a filter is given nil for a null and plain tables (no null sentinel); add_filter's second parameter is named filter (it was fn).
local template = require("lib:template")

-- compile: what is wrong with the source
local _, syntax = template.compile("{% if %}")
assert(syntax ~= nil and syntax.kind == "syntax" and syntax.op == "lib:template.compile")
assert(tostring(syntax) == "lib:template.compile: syntax: " .. syntax.reason and syntax.message == syntax.reason)
local _, deep = template.compile(string.rep("{% if true %}", 25) .. "x" .. string.rep("{% endif %}", 25))
assert(deep ~= nil and deep.kind == "too_complex" and deep.reason == "blocks are nested more than 24 deep")
local _, operators = template.compile("{{ 1" .. string.rep(" + 1", 49) .. " }}")
assert(operators ~= nil and operators.kind == "too_complex" and operators.reason == "more than 48 operators in one tag")
for _, source in { '{% include "other" %}', '{% extends "base" %}', "{% macro m() %}1{% endmacro m %}{{ 1 + self::m() }}" } do
	local _, unsupported = template.compile(source)
	assert(unsupported ~= nil and unsupported.kind == "unsupported", source)
end

-- the one-shot render has the same three, and its own
local function render_kind(source: string, context: { [string]: unknown }?): string
	local rendered, err = template.render(source, context or {})
	assert(rendered == nil and err ~= nil and err.op == "lib:template.render", source)
	return err.kind
end
assert(render_kind("{% for x in xs %}") == "syntax")
assert(render_kind(string.rep("{% if true %}", 25) .. "x" .. string.rep("{% endif %}", 25)) == "too_complex")
assert(render_kind('{% include "other" %}') == "unsupported")
assert(render_kind("{{ missing }}", { other = 1 }) == "failed")
assert(render_kind("{{ x | nope }}", { x = 1 }) == "failed")
assert(render_kind("{% macro f(n) %}{{ self::f(n=n) }}{% endmacro f %}{{ self::f(n=1) }}") == "recursion_limit")
assert(render_kind("{{ (0 - 9223372036854775807 - 1) % -1 }}") == "engine_panic")
local _, limit = template.render("{% macro f(n) %}{{ self::f(n=n) }}{% endmacro f %}{{ self::f(n=1) }}", {})
assert(limit ~= nil and limit.kind == "recursion_limit" and limit.reason:find("more than 20 levels deep", 1, true))

-- a compiled template fails on its data alone
local compiled = assert(template.compile("{{ who }}"))
local _, missing = compiled:render({})
assert(missing ~= nil and missing.kind == "failed" and missing.op == "lib:template.Template:render")
assert(missing.reason:find("who", 1, true))

-- filters: the name, what one is given, and a render inside one
local _, unnamed = template.add_filter("", function(value)
	return value
end)
assert(unnamed ~= nil and unnamed.kind == "empty_name" and tostring(unnamed) == "lib:template.add_filter: empty_name: filter name must be non-empty")
assert(template.add_filter("describe", function(value, args)
	return `{type(value)}/{type(args.flag)}/{type(args.items)}`
end) == true)
assert(template.render("{{ v | describe(flag=true, items=[1, 2]) }}", { v = { 1, 2 } }) == "table/boolean/table")
local inner_kind: string? = nil
template.add_filter("renders", function(value)
	local _, err = template.render("inner", {})
	inner_kind = if err then err.kind else nil
	return value
end)
assert(template.render("{{ 1 | renders }}", {}) == "1" and inner_kind == "recursive_render")
template.add_filter("bytes", function()
	return buffer.create(2)
end)
local _, not_plain = template.render("{{ 1 | bytes }}", {})
assert(not_plain ~= nil and not_plain.kind == "failed" and not_plain.reason:find("filter 'bytes' failed", 1, true))

-- arguments of the wrong shape are refused, with their place
local function refused(place: string, call: () -> ...unknown)
	local ok, err = pcall(call)
	assert(not ok and tostring(err):find(place, 1, true), tostring(err))
end
refused("lib:template.render: argument #2 'context'", function()
	return template.render("{{ x }}", 42 :: any)
end)
refused("lib:template.render: argument #2 'context'", function()
	return template.render("{{ x }}", { 1, 2 } :: any)
end)
refused("lib:template.render: argument #2 'context'", function()
	return template.render("x", { data = { buffer.create(2) } })
end)
refused("lib:template.render: argument #3 'options'", function()
	return template.render("x", {}, { autoescape = "yes" } :: any)
end)
refused("lib:template.render: argument #3 'options'", function()
	return template.render("x", {}, { escape = true } :: any)
end)
refused("lib:template.Template:render: argument #1 'context'", function()
	return compiled:render(42 :: any)
end)
refused("lib:template.add_filter: argument #2 'filter'", function()
	return template.add_filter("f", "upper" :: any)
end)
refused("lib:template.compile: takes 1 argument, got more", function()
	return (template.compile :: any)("x", {})
end)
