--!strict
--
-- Type definitions for `lib:diff` — paired with `src/lua/lib/diff.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Algorithm choice for line / word / char diffs and `unified`.

    - `"patience"` (default) — anchors on unique lines; produces more
      human- and LLM-readable diffs. O(N log N) typical.
    - `"myers"` — classic shortest-edit-script. O(ND) worst-case.

    A third algorithm option is intentionally not exposed — patience and
    Myers cover the practical surface.
]=]
export type DiffAlgorithm = "patience" | "myers"

--[=[
    Options accepted by `diff.lines`, `diff.words`, and `diff.chars`.
]=]
export type DiffOpts = {
    algorithm: DiffAlgorithm?,
}

--[=[
    Options accepted by `diff.unified`.

    - `algorithm` — same as `DiffOpts`.
    - `context` — number of equal lines around each hunk. Default `3`.
    - `old_label` — header label for the old side. Default `"a"`.
    - `new_label` — header label for the new side. Default `"b"`.
]=]
export type UnifiedOpts = {
    algorithm: DiffAlgorithm?,
    context: number?,
    old_label: string?,
    new_label: string?,
}

--[=[
    A single entry in a change list. `op` is always one of `"equal"`,
    `"delete"`, `"insert"` — no other values are emitted. `value` is the
    raw chunk (line, word, or char depending on the source function);
    line-level values include their trailing newline if the input did.
]=]
export type Change = {
    op: "equal" | "delete" | "insert",
    value: string,
}

--[=[
    One hunk in a unified diff result. `old_start` and `new_start` are
    1-indexed line numbers in the original / new files.

    - `old_count` — count of `equal` + `delete` lines in this hunk.
    - `new_count` — count of `equal` + `insert` lines in this hunk.
    - `lines` — the body of the hunk in order.
]=]
export type Hunk = {
    old_start: number,
    old_count: number,
    new_start: number,
    new_count: number,
    lines: { Change },
}

--[=[
    Result returned by `diff.unified` and `diff.parse_unified`. Use
    `:format()` for the unified diff string (suitable for `diff.apply`
    or display); read `.hunks` directly for programmatic access.

    `old_label` / `new_label` carry the `--- a/...` / `+++ b/...` header
    labels — preserved verbatim across format → parse round-trips.
]=]
export type UnifiedResult = {
    hunks: { Hunk },
    old_label: string,
    new_label: string,
    format: (self: UnifiedResult) -> string,
}

--[=[
    The module table returned by `require("lib:diff")`.
]=]
export type DiffModule = {
    --[=[
        Line-level change list. Default algorithm is patience.

        ```lua
        local changes = diff.lines("a\nb\nc\n", "a\nB\nc\n")
        --> { {op="equal", value="a\n"}, {op="delete", value="b\n"},
        --    {op="insert", value="B\n"}, {op="equal", value="c\n"} }
        ```

        Returns `(changes, nil)` on success or `(nil, error_string)` on
        invalid options.
    ]=]
    lines: (old: string, new: string, opts: DiffOpts?) -> ({ Change }?, string?),

    --[=[
        Word-level change list. Tokenises on whitespace boundaries.
    ]=]
    words: (old: string, new: string, opts: DiffOpts?) -> ({ Change }?, string?),

    --[=[
        Character-level change list. Operates on Unicode codepoints —
        byte-level precision escape hatch for non-line-structured edits.
    ]=]
    chars: (old: string, new: string, opts: DiffOpts?) -> ({ Change }?, string?),

    --[=[
        Compute a unified diff. Returns a structured result with `.hunks`
        and a `:format()` method that produces the unified diff string.

        ```lua
        local result = diff.unified(old, new, { context = 3 })
        print(result:format())
        for _, hunk in ipairs(result.hunks) do
            -- ...
        end
        ```
    ]=]
    unified: (old: string, new: string, opts: UnifiedOpts?) -> (UnifiedResult?, string?),

    --[=[
        Parse a unified diff string back into the same structured result
        shape as `diff.unified`. Round-trip invariant:
        `parse_unified(result:format())` is structurally identical to
        `result`.

        Error messages include the patch line number where parsing failed:

        - `"diff.parse_unified: not a unified diff"`
        - `"diff.parse_unified: truncated patch: ..."`
        - `"diff.parse_unified: invalid hunk header at line 5: ..."`
    ]=]
    parse_unified: (patch: string) -> (UnifiedResult?, string?),

    --[=[
        Apply a unified patch string to original text. Requires **exact
        context match** by default — fuzzy matching is not supported and
        would silently corrupt text.

        Distinct failure modes (each returns `nil, error_string`):

        - Malformed patch — parser error with patch line number.
        - Context mismatch — hunk context lines do not match the original.
        - Hunk overrun — patch references lines past the end of original.

        Round-trip invariant: `apply(old, unified(old, new):format()) == new`.
    ]=]
    apply: (original: string, patch: string) -> (string?, string?),

    --[=[
        Apply a structured change list directly. Validates that the
        equal + delete lines of the change list reconstruct `original`
        before emitting equal + insert.

        Round-trip invariant: `apply_changes(old, lines(old, new)) == new`.
    ]=]
    apply_changes: (original: string, changes: { Change }) -> (string?, string?),

    --[=[
        Edit-ratio similarity score in `[0.0, 1.0]`.

        Contract:
        - `similarity(a, a)` returns exactly `1.0` (including empty input).
        - `similarity("", x)` returns `0.0` for non-empty `x`.
        - `similarity(a, b) == similarity(b, a)` — symmetry holds.
        - Result is `2 * matches / (len(a) + len(b))` per Python's
          `SequenceMatcher.ratio()` semantics.

        Internally computes a character-level diff with patience.
        Precision: IEEE-754 double, no documented decimal-place guarantee.
    ]=]
    similarity: (a: string, b: string) -> number,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local diff = require("lib:diff")` resolves `diff` to `DiffModule`.
declare function require(name: "lib:diff"): DiffModule
