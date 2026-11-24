--!strict
-- lib:template as a script sees it: its answers (template.json), then what it asserts.
local template = require("lib:template")

local out = {}

-- a failure is `(nil, err)`
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
  compiled:render { v = 1 },
  compiled:render { v = "two" },
  compiled:render { v = true },
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
local before_failed = failed(before:render { s = "x" })
template.add_filter("later", function(value)
  return value .. "!"
end)
out.filters = {
  template.render("{{ s | shout }}", { s = "loud" }),
  template.render('{{ s | with_args(suffix="end") }}', { s = "core" }),
  template.render("{{ s | wrap }}", { s = "x" }, { autoescape = true }),
  template.render("{{ s | wrap | safe }}", { s = "x" }, { autoescape = true }),
  before_failed,
  before:render { s = "x" },
} :: { any }

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
local fact =
  "{% macro fact(n) %}{% if n <= 1 %}1{% else %}{{ n }}*{{ self::fact(n=n-1) }}{% endif %}{% endmacro fact %}"
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
  assert(template.compile("{{ x }}")):render { x = "compiled" },
}

-- what it asserts
do
  -- compile: what is wrong with the source
  local _, syntax = template.compile("{% if %}")
  assert(syntax ~= nil and syntax.kind == "syntax" and syntax.op == "lib:template.compile")
  assert(tostring(syntax) == "lib:template.compile: syntax: " .. syntax.reason and syntax.message == syntax.reason)
  local _, deep = template.compile(string.rep("{% if true %}", 25) .. "x" .. string.rep("{% endif %}", 25))
  assert(deep ~= nil and deep.kind == "too_complex" and deep.reason == "blocks are nested more than 24 deep")
  local _, operators = template.compile("{{ 1" .. string.rep(" + 1", 49) .. " }}")
  assert(
    operators ~= nil and operators.kind == "too_complex" and operators.reason == "more than 48 operators in one tag"
  )
  for _, source in
    { '{% include "other" %}', '{% extends "base" %}', "{% macro m() %}1{% endmacro m %}{{ 1 + self::m() }}" }
  do
    local _, unsupported = template.compile(source)
    assert(unsupported ~= nil and unsupported.kind == "unsupported", source)
  end

  -- the one-shot render has the same three, and its own
  local function render_kind(source: string, context: { [string]: any }?): string
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
  local _, missing = compiled:render {}
  assert(missing ~= nil and missing.kind == "failed" and missing.op == "lib:template.Template:render")
  assert(missing.reason:find("who", 1, true))

  -- filters: the name, what one is given, and a render inside one
  local _, unnamed = template.add_filter("", function(value)
    return value
  end)
  assert(
    unnamed ~= nil
      and unnamed.kind == "empty_name"
      and tostring(unnamed) == "lib:template.add_filter: empty_name: filter name must be non-empty"
  )
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
end

return out
