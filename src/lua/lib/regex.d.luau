--!strict
--
-- Type definitions for `lib:regex` — paired with `src/lua/lib/regex.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Engine
--
-- Backed by Rust's `regex` crate — **linear-time, no catastrophic
-- backtracking**. Lookaround (`(?=…)`, `(?<=…)`) and backreferences
-- (`\1`) are deliberately not supported; the linear-time guarantee is
-- the design point.
--
-- ## Flags
--
-- Letter flags (Perl/Python/JS style), passed at compile time:
--
-- | Flag | Effect |
-- |------|--------|
-- | `i`  | case-insensitive |
-- | `m`  | multi-line — `^` / `$` match at line boundaries |
-- | `s`  | dot-all — `.` matches `\n` |
-- | `x`  | extended — whitespace and `#`-comments ignored in pattern |
--
-- Either pass them to the flat function / `Pattern.new`, or use inline
-- syntax (`(?ims)…`) per the upstream regex crate.
--
-- Unicode matching is on by default and cannot be disabled.
--
-- ## Positions
--
-- All `start` / `finish` fields are 1-based **byte** offsets — same
-- convention as `string.find`. An empty match collapses to
-- `(start, start - 1)` so `string.sub` round-trips correctly.
--
-- ## Replacement syntax
--
-- Replacement strings use the `regex` crate's grammar:
--
-- - `$0` — whole match
-- - `$1`, `$2`, …  — positional captures
-- - `${name}` — named capture
-- - `$$` — literal `$`
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors will
-- flag these lines as errors. luau-lsp consumes them correctly.

--[=[
    Flag string for `Pattern.new` and the flat helpers.

    Each character is one independent flag — combine freely (`"im"`,
    `"isx"`, `""`). Unknown characters raise.
]=]
export type Flags = string

--[=[
    A single match against a haystack. Returned by `find`, and each
    element of `find_all`.
]=]
export type Match = {
    --[=[ The whole matched substring. ]=]
    text: string,
    --[=[ 1-based byte offset of the first byte of the match. ]=]
    start: number,
    --[=[ 1-based byte offset of the last byte of the match (inclusive).
        Empty matches collapse to `finish = start - 1`. ]=]
    finish: number,
    --[=[ Positional captures, 1-based. Group 0 (whole match) is on
        `text` / `start` / `finish` and is **not** repeated here.

        Optional groups that did not match are `false` rather than `nil`,
        so the array stays dense — `#m.groups` and `ipairs(m.groups)`
        work the way callers expect (Lua leaves both undefined on tables
        with mid-array `nil` holes). Test for unmatched with `== false`,
        not `nil`. ]=]
    groups: { string | false },
    --[=[ Named captures keyed by `(?P<name>…)`. Groups that did not match
        are absent rather than `nil` — `pairs(m.named)` only iterates
        matched names. ]=]
    named: { [string]: string },
}

--[=[
    Compiled regular expression. Construct with `regex.Pattern.new`.

    Safe to store in module-scope locals and reuse — no hidden mutable
    state. `tostring(p)` returns the original pattern source.

    ```lua
    local regex = require("lib:regex")
    local p = regex.Pattern.new("\\b\\w{5}\\b", "i")
    for _, m in ipairs(p:find_all("Hello World")) do
        print(m.text)
    end
    ```
]=]
export type Pattern = {
    --[=[
        True iff `text` contains at least one match.
    ]=]
    is_match: (self: Pattern, text: string) -> boolean,

    --[=[
        First match in `text`, or `nil` if none.
    ]=]
    find: (self: Pattern, text: string) -> Match?,

    --[=[
        Every non-overlapping match in `text`, in order. Empty when
        nothing matches.
    ]=]
    find_all: (self: Pattern, text: string) -> { Match },

    --[=[
        Replace the **first** match in `text` with `replacement`. Uses the
        regex crate's `$0` / `$1` / `${name}` substitution grammar.
    ]=]
    replace: (self: Pattern, text: string, replacement: string) -> string,

    --[=[
        Replace **every** match. Substitution grammar as `replace`.
    ]=]
    replace_all: (self: Pattern, text: string, replacement: string) -> string,

    --[=[
        Split `text` on every match.

        `limit` caps the number of returned parts: the last element
        holds the unsplit remainder. `0` or `nil` means unlimited.
        Negative values produce `(nil, "regex.split: limit must be >= 0")`.

        Leading or trailing matches produce empty string elements (same
        as Rust's `Regex::split`).
    ]=]
    split: (self: Pattern, text: string, limit: number?) -> ({ string }?, string?),
}

--[=[
    Sub-table holding the `Pattern` constructor.
]=]
export type PatternConstructor = {
    --[=[
        Compile `pattern`. Returns `(Pattern, nil)` on success or
        `(nil, "regex.Pattern.new: ...")` on a grammar error or an
        unknown flag character.

        ```lua
        local p = regex.Pattern.new("\\d+", "i")
        ```
    ]=]
    new: (pattern: string, flags: Flags?) -> (Pattern?, string?),
}

--[=[
    The module table returned by `require("lib:regex")`.
]=]
export type RegexModule = {
    Pattern: PatternConstructor,

    --[=[
        Compile-and-test in one call. Returns `(boolean, nil)` on success
        or `(nil, "regex.is_match: ...")` on a pattern error.

        ```lua
        regex.is_match("\\d+", "abc 123")    -- true
        ```
    ]=]
    is_match: (pattern: string, text: string, flags: Flags?) -> (boolean?, string?),

    --[=[
        Compile-and-find-first in one call. Returns the `Match` table on a
        hit, `(nil, nil)` on a clean miss, `(nil, "regex.find: ...")` on a
        pattern error.
    ]=]
    find: (pattern: string, text: string, flags: Flags?) -> (Match?, string?),

    --[=[
        Compile-and-find-all in one call. Returns `({ Match }, nil)`
        (possibly empty) on success or `(nil, "regex.find_all: ...")`
        on a pattern error.
    ]=]
    find_all: (pattern: string, text: string, flags: Flags?) -> ({ Match }?, string?),

    --[=[
        Replace the first match. Substitution grammar uses `$0`, `$1`,
        `${name}` per the regex crate.
    ]=]
    replace: (
        pattern: string,
        text: string,
        replacement: string,
        flags: Flags?
    ) -> (string?, string?),

    --[=[
        Replace every match. Substitution grammar as `replace`.
    ]=]
    replace_all: (
        pattern: string,
        text: string,
        replacement: string,
        flags: Flags?
    ) -> (string?, string?),

    --[=[
        Split `text` on every match of `pattern`. `limit` caps the number
        of returned parts; the last element holds the unsplit remainder.
        `0` or `nil` means unlimited. Negative limits raise.
    ]=]
    split: (
        pattern: string,
        text: string,
        limit: number?,
        flags: Flags?
    ) -> ({ string }?, string?),

    --[=[
        Escape every regex metacharacter in `text`. The result, used as a
        pattern, matches `text` literally.

        ```lua
        regex.escape("1.2.3+build")  -- "1\\.2\\.3\\+build"
        ```
    ]=]
    escape: (text: string) -> string,
}

declare function require(name: "lib:regex"): RegexModule
