---
status: accepted
created: 2026-05-19
implemented-in:
---

# 0011 — `lib:text` — String utilities and case conversion

## Summary

`lib:text` exposes case conversion (camel / snake / kebab / pascal / title / constant), greedy word wrap, codepoint-based edit distance, fuzzy `closest` ranking, display-width-aware truncation and padding, and URL-safe slugification. Backed by `heck` (case detection + acronym handling), `strsim` (Levenshtein), `unicode-width` (display columns), and `deunicode` (diacritic stripping for slugify).

## Motivation

Lua's built-in `string` library covers byte operations (`sub`, `upper`, `lower`, `find`, `gsub`) but stops well short of what scripts that emit code, format CLI output, or fuzzy-match user input need: case conversion that respects acronyms, terminal-correct padding for CJK output, edit distance, and slugification.

Cross-runtime survey:

| Runtime | Module | Notes |
|---|---|---|
| Deno | `@std/text` | `toCamelCase`, `toSnakeCase`, `toKebabCase`, `levenshteinDistance`, `closestString`, `wordWrap` |
| Python | `textwrap`, `difflib` | `wrap`, `get_close_matches`, no case conversion in stdlib |
| Go | `strings` | basic only; case conversion lives in third-party (`iancoleman/strcase`) |
| Rust | `heck` (de-facto), `strsim`, `unicode-width` | The three crates we vendor |

Concrete scenarios:

- Code generation: emit `getUserId` from `user_id` (or vice versa) without hand-rolling a tokeniser
- LLM-output formatting: fix near-miss identifier matches via `closest`
- CLI output: pad and wrap with CJK display width respected
- URL generation: slugify titles to ASCII-safe paths
- Spell-check / autocorrect: `levenshtein` + `closest`

## Detailed design

### Luau API

```lua
local text = require("lib:text")

-- Case conversion (acronym boundary detection via heck)
text.to_camel_case("hello_world")      --> "helloWorld"
text.to_snake_case("helloWorld")       --> "hello_world"
text.to_kebab_case("helloWorld")       --> "hello-world"
text.to_pascal_case("hello_world")     --> "HelloWorld"
text.to_title_case("hello_world")      --> "Hello World"
text.to_constant_case("helloWorld")    --> "HELLO_WORLD"

-- Word wrap
text.word_wrap("long text...", 80)
text.word_wrap("long text...", 80, { break_words = true, preserve_newlines = true })

-- Edit distance / fuzzy match
text.levenshtein("kitten", "sitting")  --> 3
text.closest("teh", { "the", "tea", "ten" })
-- --> { {match="tea", distance=1}, {match="ten", distance=1}, {match="the", distance=2} }
text.closest("teh", { "the", "tea", "ten" }, { limit = 2 })

-- Truncation
text.truncate("hello world", 8)                        --> "hello..."
text.truncate("hello world", 8, { boundary = "word" }) --> "hello..."

-- Padding (display-width-aware)
text.pad_left("42", 5)            --> "   42"
text.pad_left("42", 5, "0")       --> "00042"
text.pad_right("name", 10)        --> "name      "
text.center("title", 11)          --> "   title   "

-- Slugify
text.slugify("Hello, World!")     --> "hello-world"
text.slugify("Café résumé")       --> "cafe-resume"
```

### Function signatures

**`text.to_camel_case(text)`, `text.to_snake_case`, `text.to_kebab_case`, `text.to_pascal_case`, `text.to_title_case`, `text.to_constant_case`** — infallible, return `string`.

**`text.word_wrap(text, width, opts?) → (string, nil) | (nil, string)`**

| Option | Type | Default | Description |
|---|---|---|---|
| `break_words` | `boolean` | `false` | Split words longer than `width` instead of letting them overflow |
| `preserve_newlines` | `boolean` | `true` | Existing `\n` are paragraph boundaries; if `false`, they are flattened to spaces |

`width` must be `> 0` — `width = 0` returns `(nil, "text.word_wrap: width must be > 0")`. `indent` / `subsequent_indent` deferred to v2 (composable via string concatenation for now).

**`text.levenshtein(a, b) → number`** — infallible; codepoint-based edit distance via `strsim::levenshtein`. Symmetric: `levenshtein(a, b) == levenshtein(b, a)`.

**`text.closest(input, candidates, opts?) → ({Match}?, string?)`**

Returns scored matches sorted by ascending distance, ties broken alphabetically.

| Option | Type | Default | Description |
|---|---|---|---|
| `limit` | `number?` | `nil` | Cap the result list. `limit = 0` returns an empty table. |

- Empty `candidates` returns `nil` — "no candidates to score" is distinct from "no candidates matched well."
- Exact match present: distance `0` row appears first.
- All ties: alphabetical ascending. Deterministic across runs.

**`text.truncate(text, max_len, opts?) → (string?, string?)`**

The ellipsis (`"..."`) counts toward `max_len`. When `max_len < 3` (ellipsis cannot fit) the ellipsis is dropped and `text` is truncated to `max_len` display columns directly.

| Option | Type | Default | Description |
|---|---|---|---|
| `boundary` | `"char" \| "word"` | `"char"` | `"word"` truncates at the last whitespace before the budget |

| Input | `max_len` | Opts | Output |
|---|---|---|---|
| `"hello world"` | 8 | default | `"hello..."` |
| `"hello world"` | 8 | `{boundary="word"}` | `"hello..."` |
| `"hi"` | 8 | default | `"hi"` |
| `"hello"` | 3 | default | `"..."` (ellipsis exactly fills) |
| `"hello"` | 2 | default | `"he"` (ellipsis dropped) |
| `""` | 5 | default | `""` |

**`text.pad_left(text, width, fill_char?) → (string?, string?)`**, **`text.pad_right`**, **`text.center`**

Display-column padding via `unicode-width`. `fill_char` defaults to `" "`; if provided it **must** be exactly one codepoint of display width 1. Multi-codepoint, empty, zero-width, or double-width fill chars all return a tuple error. `center` distributes uneven padding with the extra column on the right.

If `text` already meets or exceeds `width`, returns `text` unchanged.

**`text.slugify(text) → string`**

URL-safe slug: ASCII transliteration via `deunicode` (so `café → cafe`), lowercase, runs of non-alphanumerics collapse to a single hyphen, leading/trailing hyphens trimmed. Empty stays empty.

### Case conversion — acronym boundary detection

Backed by `heck`. Pinned test vectors:

| Input | `snake` | `camel` | `pascal` | `kebab` | `constant` |
|---|---|---|---|---|---|
| `"helloWorld"` | `"hello_world"` | `"helloWorld"` | `"HelloWorld"` | `"hello-world"` | `"HELLO_WORLD"` |
| `"HTTPSConnection"` | `"https_connection"` | `"httpsConnection"` | `"HttpsConnection"` | `"https-connection"` | `"HTTPS_CONNECTION"` |
| `"getHTTPSURL"` | `"get_httpsurl"` | `"getHttpsurl"` | `"GetHttpsurl"` | `"get-httpsurl"` | `"GET_HTTPSURL"` |
| `"XMLParser"` | `"xml_parser"` | `"xmlParser"` | `"XmlParser"` | `"xml-parser"` | `"XML_PARSER"` |
| `""` | `""` | `""` | `""` | `""` | `""` |
| `"a"` | `"a"` | `"a"` | `"A"` | `"a"` | `"A"` |
| `"already_snake"` | `"already_snake"` | `"alreadySnake"` | `"AlreadySnake"` | `"already-snake"` | `"ALREADY_SNAKE"` |
| `"kebab-case"` | `"kebab_case"` | `"kebabCase"` | `"KebabCase"` | `"kebab-case"` | `"KEBAB_CASE"` |
| `"  spaces  "` | `"spaces"` | `"spaces"` | `"Spaces"` | `"spaces"` | `"SPACES"` |
| `"123numeric"` | `"123numeric"` | `"123numeric"` | `"123numeric"` | `"123numeric"` | `"123NUMERIC"` |
| `"with.dots.in"` | `"with_dots_in"` | `"withDotsIn"` | `"WithDotsIn"` | `"with-dots-in"` | `"WITH_DOTS_IN"` |
| `"MIXed_CASE-stuff"` | `"mi_xed_case_stuff"` | `"miXedCaseStuff"` | `"MiXedCaseStuff"` | `"mi-xed-case-stuff"` | `"MI_XED_CASE_STUFF"` |

Notes:

- **`getHTTPSURL`** — heck treats the trailing all-caps `HTTPSURL` as one acronym word (no boundary between adjacent uppercase runs). Snake result is `get_httpsurl`, not `get_https_url`. This is consistent across heck's case conversions and matches the rule "split before a lowercase that follows an uppercase," which can't see backward through a run of uppercase.
- **`MIXed`** — splits between `MI` and `Xed` (UPPER-UPPER-lower → break before the trailing upper).
- **Whitespace and `.` `-` `_`** — all are word boundaries; heck normalizes them away.
- **Digits** — same character class as letters within a word: `123numeric` is a single token (no digit-letter boundary).

### `to_title_case` — acronym output

`"HTTPSConnection".to_title_case() == "Https Connection"` (heck lowercases the rest of an acronym after the first letter). Apostrophes split on their non-alphanumeric class.

### `word_wrap` test matrix

| Input | Width | Opts | Output |
|---|---|---|---|
| `"hello world"` | 80 | default | `"hello world"` |
| `"hello world"` | 5 | default | `"hello\nworld"` |
| `"abcdefghij"` | 5 | `{break_words=true}` | `"abcde\nfghij"` |
| `"abcdefghij"` | 5 | `{break_words=false}` | `"abcdefghij"` (overflows) |
| `"line1\nline2"` | 80 | `{preserve_newlines=true}` | `"line1\nline2"` |
| `"line1\nline2"` | 80 | `{preserve_newlines=false}` | `"line1 line2"` |
| `""` | 80 | default | `""` |
| `"a"` | 0 | default | `(nil, "text.word_wrap: width must be > 0")` |

### `closest` — scored result contract

```lua
text.closest("teh", { "the", "tea", "ten" })
-- { { match = "tea", distance = 1 },
--   { match = "ten", distance = 1 },
--   { match = "the", distance = 2 } }
```

- Empty `candidates`: `nil` (not empty table — distinguishable from `limit = 0`)
- Exact match: distance `0` row sorts first
- All candidates equidistant: alphabetical ascending
- `limit = 0`: empty table
- `limit < 0` or non-integer: `(nil, "text.closest: 'limit' must be a non-negative integer")`

### `truncate` — ellipsis accounting

The ellipsis counts toward `max_len`. Truncation operates on display columns (`unicode-width`), not codepoints, so CJK characters are correctly accounted for at terminal output width.

### `pad_*` and `center` — Unicode width

Padding equalises **display width**, not codepoint count. `"あ"` (display width 2) padded to 5 columns receives 3 columns of fill, not 4 characters.

Fill char constraints:

- Exactly one codepoint
- Display width exactly 1

Violations return `(nil, "text.pad_left: fill char must …")`.

### `slugify` — normalisation contract

| Input | Output |
|---|---|
| `"Hello, World!"` | `"hello-world"` |
| `"  leading trailing  "` | `"leading-trailing"` |
| `"---multiple---hyphens---"` | `"multiple-hyphens"` |
| `""` | `""` |
| `"Café résumé"` | `"cafe-resume"` |
| `"日本語"` | `"ri-ben-yu"` |
| `"Привет мир"` | `"privet-mir"` |
| `"hello 😀"` | `"hello-grinning"` |

Transliteration policy: **`deunicode` full Unicode → ASCII**, not just diacritic strip. Concretely:

- Latin diacritics drop (`café` → `cafe`, `München` → `munchen`)
- CJK becomes pinyin-style ASCII (`日本語` → `ri-ben-yu`)
- Cyrillic becomes Latin (`Привет` → `privet`)
- Emoji become their English names (`😀` → `grinning`)

This is the right default for routing slugs (URLs, file names, search keys) — every input produces a routable ASCII path. Applications wanting internationalised slugs can construct their own pipeline from `text.to_kebab_case` on the lowercase original.

### `levenshtein` — known-value verification

| `a` | `b` | Distance |
|---|---|---|
| `"kitten"` | `"sitting"` | 3 |
| `""` | `"abc"` | 3 |
| `"abc"` | `""` | 3 |
| `"abc"` | `"abc"` | 0 |
| `""` | `""` | 0 |

Symmetry holds because `strsim::levenshtein` implements the standard metric.

## Acceptance criteria

1. `to_camel_case`, `to_snake_case`, `to_kebab_case`, `to_pascal_case`, `to_title_case`, `to_constant_case` via `heck` ✅
2. Pinned test vectors for acronym handling and edge inputs ✅
3. `word_wrap(text, width, opts?)` with `break_words` and `preserve_newlines`; `width = 0` errors ✅
4. `levenshtein(a, b)` codepoint-based; symmetry verified ✅
5. `closest(input, candidates, opts?)` with ascending-distance ranking, alphabetical tie-break, `limit`, empty-candidates → `nil`, `limit = 0` → empty table ✅
6. `truncate(text, max_len, opts?)` with `boundary = "word"`; ellipsis counts toward `max_len`; behaviour pinned for `max_len < 3` ✅
7. `pad_left`, `pad_right`, `center` display-width-correct via `unicode-width`; multi/empty/double-width fill rejected ✅
8. `slugify` strips diacritics, lowercases ASCII, collapses non-alphanumeric runs to single hyphens, trims ends ✅
9. Display-width-aware functions (wrap, truncate, pad, center) all use `unicode-width` ✅
10. Module docstring documents the codepoint-vs-display-width split ✅
11. Backed by `heck`, `strsim`, `unicode-width`, `deunicode` ✅
12. `(nil, error)` tuple on invalid input where applicable; case conversions are infallible ✅
13. Spec, Luau integration tests, Rust unit tests, and `.d.luau` type stub ✅

## Out of scope

- `indent` / `subsequent_indent` for `word_wrap` (deferred to v2)
- Regex-based text operations (covered by future `lib:regex`)
- Configurable slugify (diacritic preservation, custom separator) — applications can compose `to_kebab_case` + lowercase for that today
