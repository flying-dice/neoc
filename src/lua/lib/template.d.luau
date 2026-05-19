--!strict
--
-- Type definitions for `lib:template` — paired with `src/lua/lib/template.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface is
-- reviewed alongside the implementation.
--
-- ## Engine
--
-- Tera 1.x — Jinja2/Django-style syntax. Pipelines (`{{ s | upper }}`),
-- loops (`{% for x in xs %}...{% endfor %}`), conditionals
-- (`{% if cond %}...{% else %}...{% endif %}`), whitespace control
-- (`{%- -%}`).
--
-- ## Defaults
--
-- - **Autoescape off by default.** Opt in per render with
--   `{ autoescape = true }` — escapes HTML special characters for output that
--   ends up in HTML/XML. Off-by-default keeps plain-text and code-generation
--   output untouched.
-- - **Undefined variables error.** `{{ missing }}` returns
--   `(nil, "template.render: …")` rather than silently producing `""`.
--
-- ## Thread safety
--
-- Compiled `Template` userdata wraps an `Arc<Mutex<...>>` engine handle and
-- is safe to share across `std:task` coroutines (same engine, same VM).
-- `std:thread` workers run a fresh engine per worker and `Template` is not
-- Sendable — cross-worker handoff is not supported.
--
-- ## Re-entrant render
--
-- Filters must not call `template.render` or `Template:render` from inside
-- their callback. The engine lock is non-reentrant; the nested call returns
-- `(nil, "...: recursive render not supported")` rather than deadlocking.
--
-- ## Custom filters
--
-- Registered filters live on the engine — visible to every render call,
-- including renders on `Template` userdata that was compiled *before* the
-- filter was registered.
--
-- Filter signature: `(value, args) -> result`. Filter output is HTML-escaped
-- when `{ autoescape = true }` is set. For unescaped output, chain tera's
-- built-in `safe` filter in the template: `{{ x | shout | safe }}`.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag these lines as errors. luau-lsp consumes them correctly — don't remove
-- them to "fix" the squiggles.

export type TemplateFilter = (value: any, args: { [string]: any }) -> any

--[=[
    Render options. `autoescape` toggles HTML escaping for the rendered output;
    custom filter output is escaped along with substituted variables when on.
]=]
export type RenderOptions = {
    autoescape: boolean?,
}

--[=[
    A compiled template. Reusable — call `:render(ctx)` as many times as you
    like. Safe to share across `std:task` coroutines (same engine, same VM).
]=]
export type Template = {
    --[=[
        Render the compiled template with `context` as the variable map.

        Returns `(string, nil)` on success; `(nil, "Template.render: ...")` on
        failure (missing variable, filter error, etc.).
    ]=]
    render: (self: Template, context: { [string]: any }) -> (string?, string?),
}

--[=[
    The module table returned by `require("lib:template")`.
]=]
export type TemplateModule = {
    --[=[
        Parse `source` once, return a reusable `Template`.

        Returns `(Template, nil)` on success; `(nil, "template.compile: ...")`
        on parse error (the message includes line/column when tera produces it).

        ```lua
        local template = require("lib:template")
        local t = assert(template.compile("Hello, {{ name }}!"))
        for _, who in ipairs({ "Alice", "Bob" }) do
            print(t:render({ name = who }))
        end
        ```
    ]=]
    compile: (source: string) -> (Template?, string?),

    --[=[
        One-shot parse + render — convenience for templates rendered once.
        For repeated renders, compile once and call `:render` instead.

        Optional third arg toggles HTML autoescape:
        `template.render(src, ctx, { autoescape = true })`.

        ```lua
        template.render("Hello, {{ name }}!", { name = "world" })
        --> "Hello, world!"
        ```
    ]=]
    render: (
        source: string,
        context: { [string]: any },
        options: RenderOptions?
    ) -> (string?, string?),

    --[=[
        Register a Lua function as a custom tera filter.

        Filters become visible to every subsequent render — including renders
        on `Template` userdata compiled before the filter existed.

        Filter output is HTML-escaped when `autoescape = true`. To emit raw
        output, chain tera's built-in `safe` filter:
        `{{ x | shout | safe }}`.

        ```lua
        template.add_filter("shout", function(value, _args)
            return string.upper(tostring(value))
        end)
        template.render("{{ name | shout }}", { name = "world" })
        --> "WORLD"
        ```
    ]=]
    add_filter: (name: string, fn: TemplateFilter) -> (true?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local template = require("lib:template")` resolves to `TemplateModule`.
declare function require(name: "lib:template"): TemplateModule
