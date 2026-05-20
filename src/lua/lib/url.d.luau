--!strict
--
-- Type definitions for `lib:url` — paired with `src/lua/lib/url.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Named percent-encode set for `url.encode`.

    - `"component"` — RFC 3986 unreserved (`A-Za-z0-9-_.~`) survives,
      everything else encoded. Equivalent to JS `encodeURIComponent`. The
      default when no set is specified.
    - `"path"` — WHATWG path-percent-encode set (`?`, `` ` ``, `{`, `}` plus
      the query set below).
    - `"query"` — WHATWG query-percent-encode set (C0 controls + space, `"`,
      `#`, `<`, `>`).
    - `"fragment"` — WHATWG fragment-percent-encode set (C0 controls +
      space, `"`, `<`, `>`, `` ` ``).
]=]
export type EncodeSet = "component" | "path" | "query" | "fragment"

--[=[
    A single decoded query pair: `{ key, value }`. Both elements are decoded
    UTF-8 strings — `+` has been mapped to space and percent-escapes resolved.
]=]
export type QueryPair = { string }

--[=[
    Parsed URL. Construct with `url.Url.parse(s)` or the flat alias
    `url.parse(s)`. Immutable — every `with_*` method returns a new
    `Url`. `tostring(u)` returns the canonical (percent-encoded) form,
    equivalent to `u.href`.

    ```lua
    local u = url.parse("https://ada:secret@example.com:8443/a/b?x=1#frag")
    u.scheme    -- "https"
    u.host      -- "example.com"
    u.port      -- 8443
    u.path      -- "/a/b"
    u.query     -- "x=1"
    u.fragment  -- "frag"
    ```
]=]
export type Url = {
    --[=[ URL scheme, lowercased (`"https"`, `"file"`, …). Never `nil`. ]=]
    scheme: string,
    --[=[ Username; empty string when absent. Returned percent-encoded —
        matches JS `URL.username`. Pipe through `url.decode` if you need
        the literal value. ]=]
    username: string,
    --[=[ Password, or `nil` when absent. Returned percent-encoded —
        matches JS `URL.password`. Pipe through `url.decode` if you need
        the literal value. ]=]
    password: string?,
    --[=[ Host string, or `nil` for schemes without an authority (`file:`). ]=]
    host: string?,
    --[=[ Port as an integer, or `nil` when the URL relies on the scheme's
        default. Use `:with_port(nil)` to clear an explicit port. ]=]
    port: number?,
    --[=[ Path, always starting with `/` for URLs with an authority. ]=]
    path: string,
    --[=[ Raw query string after `?` (not including the `?` itself), or `nil`. ]=]
    query: string?,
    --[=[ Raw fragment after `#` (not including the `#`), or `nil`. ]=]
    fragment: string?,
    --[=[ Tuple-style origin (`scheme://host:port` for special schemes,
        `"null"` otherwise). ]=]
    origin: string,
    --[=[ Canonical URL string. Equivalent to `tostring(u)`. ]=]
    href: string,

    --[=[
        Resolve `reference` against this URL using RFC 3986 § 5 rules.
        Returns `(Url, nil)` on success or `(nil, "url.Url.resolve: ...")`.
    ]=]
    resolve: (self: Url, reference: string) -> (Url?, string?),

    --[=[
        Return a new `Url` with the scheme replaced. Most scheme swaps
        between special and non-special schemes are rejected by the spec —
        check the error string on failure.
    ]=]
    with_scheme: (self: Url, scheme: string) -> (Url?, string?),
    --[=[
        Return a new `Url` with the host replaced. Returns `(nil, err)` if
        the host is malformed for the scheme.
    ]=]
    with_host: (self: Url, host: string) -> (Url?, string?),
    --[=[
        Return a new `Url` with the port set (or cleared when `port` is
        `nil`). `port` must be an integer in `0..=65535`; non-integer or
        out-of-range values return `(nil, "url.Url.with_port: ...")`.
        URL types that cannot carry a port also return `(nil, err)`.
    ]=]
    with_port: (self: Url, port: number?) -> (Url?, string?),
    --[=[
        Return a new `Url` with the path replaced. Always succeeds — the
        path is re-encoded against the WHATWG path set.
    ]=]
    with_path: (self: Url, path: string) -> Url,
    --[=[
        Return a new `Url` with the query string replaced (or cleared when
        `query` is `nil`). Build the string with `encode_query` if you need
        form-encoding.

        Empty string and `nil` differ: `with_query("")` keeps the trailing
        `?` separator (`https://example.com/?`), `with_query(nil)` drops
        it (`https://example.com/`). Mirrors the underlying `url` crate
        and WHATWG behaviour. Pass `nil` when you want the URL printed
        without a `?`.
    ]=]
    with_query: (self: Url, query: string?) -> Url,
    --[=[
        Return a new `Url` with the fragment replaced (or cleared when
        `fragment` is `nil`).
    ]=]
    with_fragment: (self: Url, fragment: string?) -> Url,

    --[=[
        Query pairs in order, including any repeated keys. `?a=1&b=2&a=3`
        decodes to `{ {"a","1"}, {"b","2"}, {"a","3"} }`.
    ]=]
    query_pairs: (self: Url) -> { QueryPair },
    --[=[
        Query as a map. **Last value wins on repeated keys** —
        `?a=1&a=2` yields `{ a = "2" }`. Use `query_pairs` when this
        collapse loses information.
    ]=]
    query_map: (self: Url) -> { [string]: string },
}

--[=[
    Sub-table holding the `Url` constructor.
]=]
export type UrlConstructor = {
    --[=[
        Parse an absolute URL string. Returns `(Url, nil)` on success or
        `(nil, "url.Url.parse: ...")` if the input is not a valid URL.
    ]=]
    parse: (s: string) -> (Url?, string?),
}

--[=[
    The module table returned by `require("lib:url")`.
]=]
export type UrlModule = {
    Url: UrlConstructor,

    --[=[
        Alias for `Url.parse`. Returns `(Url, nil)` on success or
        `(nil, "url.parse: ...")` on malformed input.
    ]=]
    parse: (s: string) -> (Url?, string?),

    --[=[
        Resolve a relative reference against `base` (a `Url` or string).
        Returns `(Url, nil)` on success or `(nil, "url.resolve: ...")`.
    ]=]
    resolve: (base: Url | string, reference: string) -> (Url?, string?),

    --[=[
        Percent-encode `input` against the named `set` (defaults to
        `"component"` — the encodeURIComponent equivalent). Returns
        `(string, nil)` on success or `(nil, "url.encode: ...")` when
        `set` is not one of the named sets. Encoding itself never fails.
    ]=]
    encode: (input: string, set: EncodeSet?) -> (string?, string?),

    --[=[
        Percent-decode `input`. Returns `(string, nil)` on success or
        `(nil, "url.decode: ...")` when the decoded bytes are not valid
        UTF-8. Does *not* convert `+` to space — see `decode_query` for
        form-encoded payloads.
    ]=]
    decode: (input: string) -> (string?, string?),

    --[=[
        Form-encode an ordered list of pairs into a query string. Spaces
        in keys/values become `+`; other reserved chars are percent-
        encoded. The encoding never fails.
    ]=]
    encode_query: (pairs: { QueryPair }) -> (string?, string?),

    --[=[
        Parse an `application/x-www-form-urlencoded` payload into an
        ordered list of pairs. `+` is decoded to space; repeated keys are
        preserved. Mirrors the round-trip of `encode_query`.
    ]=]
    decode_query: (input: string) -> { QueryPair },
}

declare function require(name: "lib:url"): UrlModule
