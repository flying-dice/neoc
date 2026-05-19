--!strict
--
-- Type definitions for `lib:text` — paired with `src/lua/lib/text.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Options for `text.word_wrap`.
]=]
export type WordWrapOpts = {
    break_words: boolean?,
    preserve_newlines: boolean?,
}

--[=[
    Options for `text.closest`.
]=]
export type ClosestOpts = {
    limit: number?,
}

--[=[
    Truncation boundary mode.
    - `"char"` — truncate at any codepoint (default)
    - `"word"` — truncate at the last whitespace boundary before the limit
]=]
export type TruncateBoundary = "char" | "word"

--[=[
    Options for `text.truncate`.
]=]
export type TruncateOpts = {
    boundary: TruncateBoundary?,
}

--[=[
    A scored fuzzy-match result from `text.closest`.
]=]
export type Match = {
    match: string,
    distance: number,
}

--[=[
    The module table returned by `require("lib:text")`.

    All operations count Unicode codepoints unless otherwise noted.
    `word_wrap`, `truncate`, `pad_left`, `pad_right`, and `center` use
    `unicode-width` for terminal-correct display columns (CJK = 2 cols).
]=]
export type TextModule = {
    --[=[
        Convert to `camelCase`. Acronym boundaries (e.g. `HTTPSConnection`)
        are detected by the backing `heck` crate.

        ```lua
        text.to_camel_case("hello_world")    --> "helloWorld"
        text.to_camel_case("HTTPSConnection") --> "httpsConnection"
        ```
    ]=]
    to_camel_case: (text: string) -> string,

    --[=[
        Convert to `snake_case`.

        ```lua
        text.to_snake_case("helloWorld")     --> "hello_world"
        text.to_snake_case("XMLParser")      --> "xml_parser"
        ```
    ]=]
    to_snake_case: (text: string) -> string,

    --[=[
        Convert to `kebab-case`.
    ]=]
    to_kebab_case: (text: string) -> string,

    --[=[
        Convert to `PascalCase`.
    ]=]
    to_pascal_case: (text: string) -> string,

    --[=[
        Convert to `Title Case`.
    ]=]
    to_title_case: (text: string) -> string,

    --[=[
        Convert to `CONSTANT_CASE`.
    ]=]
    to_constant_case: (text: string) -> string,

    --[=[
        Greedy word wrap. Returns `(string, nil)` on success or
        `(nil, "text.word_wrap: ...")` on invalid input.

        Defaults: `break_words = false`, `preserve_newlines = true`.

        `width` must be > 0.

        ASCII whitespace is the only word boundary. CJK and other scripts
        without whitespace are treated as a single "word" and will overflow
        unless `break_words = true` is set — at which point the wrap respects
        display columns (CJK = 2 cols).

        ```lua
        text.word_wrap("hello world", 5)
            --> "hello\nworld", nil
        text.word_wrap("abcdefghij", 5, { break_words = true })
            --> "abcde\nfghij", nil
        text.word_wrap("あいうえお", 4, { break_words = true })
            --> "あい\nうえ\nお", nil
        ```
    ]=]
    word_wrap: (text: string, width: number, opts: WordWrapOpts?) -> (string?, string?),

    --[=[
        Codepoint-based Levenshtein edit distance.

        ```lua
        text.levenshtein("kitten", "sitting")  --> 3
        ```
    ]=]
    levenshtein: (a: string, b: string) -> number,

    --[=[
        Score each candidate against `input` by Levenshtein distance and
        return the list sorted ascending. Ties are broken alphabetically.

        Returns `nil` when `candidates` is empty. Returns an empty table
        when `opts.limit == 0`.

        ```lua
        text.closest("teh", {"the", "tea", "ten"})
            --> { {match="tea", distance=1}, {match="ten", distance=1}, {match="the", distance=2} }
        text.closest("teh", {"the", "tea", "ten"}, { limit = 2 })
        ```
    ]=]
    closest: (input: string, candidates: { string }, opts: ClosestOpts?) -> ({ Match }?, string?),

    --[=[
        Truncate `text` so its display width is at most `max_len`. The
        ellipsis (`...`) counts toward the limit. If `max_len < 3` (cannot
        fit the ellipsis) the ellipsis is dropped and the prefix is taken
        directly.

        ```lua
        text.truncate("hello world", 8)                        --> "hello..."
        text.truncate("hello world", 8, { boundary = "word" }) --> "hello..."
        text.truncate("hello", 2)                              --> "he"
        text.truncate("hello", 3)                              --> "..."
        ```
    ]=]
    truncate: (text: string, max_len: number, opts: TruncateOpts?) -> (string?, string?),

    --[=[
        Pad `text` on the left so total display width equals `width`.
        `fill_char` must be exactly one character with display width 1
        (default `" "`). Returns `text` unchanged if it already meets or
        exceeds `width`.
    ]=]
    pad_left: (text: string, width: number, fill_char: string?) -> (string?, string?),

    --[=[
        Pad `text` on the right so total display width equals `width`.
        Same constraints as `pad_left`.
    ]=]
    pad_right: (text: string, width: number, fill_char: string?) -> (string?, string?),

    --[=[
        Pad `text` on both sides so total display width equals `width`.
        Uneven padding places the extra column on the right.
    ]=]
    center: (text: string, width: number, fill_char: string?) -> (string?, string?),

    --[=[
        Convert `text` to a URL-safe ASCII slug. Backed by `deunicode`, which
        does full Unicode → ASCII transliteration (not just diacritic strip):

        - Latin diacritics drop: `café` → `cafe`, `München` → `munchen`
        - CJK becomes pinyin-style ASCII: `日本語` → `ri-ben-yu`
        - Cyrillic becomes Latin: `Привет` → `privet`
        - Emoji become their English names: `😀` → `grinning`

        After transliteration: lowercased, runs of non-alphanumerics collapse
        to a single hyphen, leading/trailing hyphens trimmed.

        ```lua
        text.slugify("Hello, World!")     --> "hello-world"
        text.slugify("---café---")        --> "cafe"
        text.slugify("日本語")            --> "ri-ben-yu"
        text.slugify("hello 😀")          --> "hello-grinning"
        ```
    ]=]
    slugify: (text: string) -> string,
}

declare function require(name: "lib:text"): TextModule
