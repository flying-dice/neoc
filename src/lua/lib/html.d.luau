--!strict
--
-- Type definitions for `lib:html` — paired with `src/lua/lib/html.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Options for `html.escape`.

    Reserved for future context-specific modes. v1 ignores all keys without
    error — `escape` always emits all 5 XML entities, which is safe in both
    content and attribute contexts.
]=]
export type HtmlEscapeOpts = {
    context: ("content" | "attribute")?,
}

--[=[
    The module table returned by `require("lib:html")`.
]=]
export type HtmlModule = {
    --[=[
        Escape the 5 XML entities (`&`, `<`, `>`, `"`, `'`) to their named
        forms (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`). Safe in both
        HTML content and HTML attribute contexts.

        The optional `opts` table is reserved for future context-specific
        modes; v1 accepts any table but ignores all keys.

        ```lua
        local html = require("lib:html")
        html.escape([[<script>alert("xss")</script>]])
        --> "&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;"
        ```
    ]=]
    escape: (input: string, opts: HtmlEscapeOpts?) -> string,

    --[=[
        Reverse the 5 XML entity escapes plus numeric character references
        (`&#N;` decimal, `&#xN;` hex). Unknown named entities and malformed
        numeric forms pass through unchanged — `unescape` is a string
        transform, not a validating parser.

        Numeric references for NUL (`&#0;`), surrogates (`&#xD800;`), and
        codepoints beyond `U+10FFFF` are passed through; emitting them
        would corrupt UTF-8 or enable NUL injection.

        ```lua
        html.unescape("&lt;b&gt;bold&lt;/b&gt;")  -- "<b>bold</b>"
        html.unescape("&notareal;")                -- "&notareal;"  (unknown, passthrough)
        html.unescape("&#65;")                     -- "A"
        ```

        Round-trip invariant: `html.unescape(html.escape(s)) == s` for all
        valid UTF-8 strings.

        Not idempotent in general — `html.unescape(html.unescape("&amp;lt;"))`
        is `"<"`, not `"&lt;"`. Each call performs exactly one entity
        expansion pass.
    ]=]
    unescape: (input: string) -> string,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local html = require("lib:html")` resolves `html` to `HtmlModule`.
declare function require(name: "lib:html"): HtmlModule
