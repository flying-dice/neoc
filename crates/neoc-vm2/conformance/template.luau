-- lib:template on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- What an error is and how a wrong context or option is refused differ on purpose: see template.vm2.luau.
local template = require("lib:template")

local out = {}

-- a failure is `(nil, err)`; what `err` is differs
local function failed(value: unknown, err: unknown): { boolean }
	return { value == nil, err ~= nil }
end

out.render = {
	template.render("Hello, {{ name }}!", { name = "world" }),
	template.render("{% for n in nums %}{{ n }},{% endfor %}", { nums = { 1, 2, 3 } }),
	template.render("{% if flag %}YES{% else %}NO{% endif %}", { flag = true }),
	template.render("{% if flag %}YES{% else %}NO{% endif %}", { flag = false }),
	template.render("{{ s | upper }}{{ t | lower }}{{ s | length }}", { s = "shout", t = "QUIET" }),
	template.render("A{%- if x %}B{%- endif %}", { x = true }),
	template.render("{{ user.name }} has {{ user.tags | length }} tags: {{ user.tags | join(sep='+') }}", {
		user = { name = "ada", tags = { "a", "b" } },
	}),
	template.render("{% for i in range(start=1, end=10, step_by=3) %}{{ i }},{% endfor %}", {}),
	template.render("{% macro hi(n) %}hi {{ n }}{% endmacro hi %}{{ self::hi(n=1) }}", {}),
	template.render("{% raw %}{{ not a tag }}{% endraw %}", {}),
	template.render("no tags at all", {}),
}
out.autoescape = {
	template.render("{{ s }}", { s = "<b>" }),
	template.render("{{ s }}", { s = "<b>" }, { autoescape = true }),
	template.render("{{ s }}", { s = "<b>" }, { autoescape = false }),
	template.render("{{ s | safe }}", { s = "<b>" }, { autoescape = true }),
}

local compiled = assert(template.compile("v={{ v }}"))
out.compiled = {
	compiled:render({ v = 1 }),
	compiled:render({ v = "two" }),
	compiled:render({ v = true }),
	tostring(compiled):sub(1, 9),
}

template.add_filter("shout", function(value, _args)
	return string.upper(tostring(value))
end)
template.add_filter("with_args", function(value, args)
	return tostring(value) .. ":" .. tostring(args.suffix)
end)
template.add_filter("wrap", function(value)
	return "<b>" .. tostring(value) .. "</b>"
end)
local before = assert(template.compile("{{ s | later }}"))
local before_failed = failed(before:render({ s = "x" }))
template.add_filter("later", function(value)
	return value .. "!"
end)
out.filters = {
	template.render("{{ s | shout }}", { s = "loud" }),
	template.render('{{ s | with_args(suffix="end") }}', { s = "core" }),
	template.render("{{ s | wrap }}", { s = "x" }, { autoescape = true }),
	template.render("{{ s | wrap | safe }}", { s = "x" }, { autoescape = true }),
	before_failed,
	before:render({ s = "x" }),
}

template.add_filter("kaboom", function()
	error("filter exploded")
end)
template.add_filter("recurse", function()
	local rendered, err = template.render("inner", {})
	if not rendered then
		error(tostring(err))
	end
	return rendered
end)
local fact = "{% macro fact(n) %}{% if n <= 1 %}1{% else %}{{ n }}*{{ self::fact(n=n-1) }}{% endif %}{% endmacro fact %}"
out.failures = {
	failed(template.render("{{ missing }}", { other = 1 })),
	failed(template.render("{% for x in xs %}", { xs = {} })),
	failed(template.compile("{% if %}")),
	failed(template.render("{{ x | kaboom }}", { x = 1 })),
	failed(template.render("{{ x | nope }}", { x = 1 })),
	failed(template.render("{{ x | recurse }}", { x = 1 })),
	failed(template.add_filter("", function()
		return ""
	end)),
	failed(template.compile('{% include "other" %}')),
	failed(template.compile('{% import "other" as o %}')),
	failed(template.compile('{% extends "base" %}')),
	failed(template.render("{% macro m() %}1{% endmacro m %}{{ 1 + self::m() }}", {})),
	failed(template.render("{% for i in range(end=10, step_by=0) %}{{ i }}{% endfor %}", {})),
	failed(template.render("{% for i in range(end=1000000000000000000) %}x{% endfor %}", {})),
	failed(template.render("{{ (0 - 9223372036854775807 - 1) % -1 }}", {})),
	failed(template.render(string.rep("{% if true %}", 25) .. "x" .. string.rep("{% endif %}", 25), {})),
	failed(template.compile("{{ 1" .. string.rep(" + 1", 49) .. " }}")),
	failed(template.compile("{{ " .. string.rep("(", 25) .. "1" .. string.rep(")", 25) .. " }}")),
	failed(template.render("{% macro f(n) %}{{ self::f(n=n) }}{% endmacro f %}{{ self::f(n=1) }}", {})),
	failed(template.render(fact .. "{{ self::fact(n=14) }}", {})),
}
-- the limits themselves render, a refused template breaks nothing after it, and the engine survives a panic
out.after = {
	template.render(string.rep("{% if true %}", 24) .. "x" .. string.rep("{% endif %}", 24), {}),
	template.render("{{ 1" .. string.rep(" + 1", 48) .. " }}", {}),
	template.render(fact .. "{{ self::fact(n=13) }}", {}),
	template.render("{{ x }}", { x = "still works" }),
	assert(template.compile("{{ x }}")):render({ x = "compiled" }),
}

return out
