--!strict
--
-- Type definitions for `lib:glob` — paired with `src/lua/lib/glob.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Entry-type filter for `scan` results.
]=]
export type EntryType = "file" | "dir" | "any"

--[=[
    Options for `glob.scan` and `Glob:scan`.
]=]
export type ScanOpts = {
    cwd: string?,
    absolute: boolean?,
    limit: number?,
    ignore: { string }?,
    follow_symlinks: boolean?,
    type: EntryType?,
}

--[=[
    Per-entry error surfaced when `scan` encounters a path it could not
    descend into (permission denied, broken symlink, etc.). The walk
    continues; the entry is reported, not swallowed.
]=]
export type ScanError = { path: string, error: string }

--[=[
    Compiled glob pattern. Construct with `glob.Glob.new(pattern)`.

    Safe to store in module-scope locals and reuse — no hidden mutable
    state. `tostring(g)` returns the original pattern string.

    **Dotfile rule.** A path is filtered if *any* path segment starts with
    `.` and the pattern does not include a segment that literally starts
    with `.` (Unix convention, matches `ripgrep`/`fd`). Examples:

    - `glob.is_match("*", ".gitignore")` → `false`
    - `glob.is_match(".git*", ".gitignore")` → `true`
    - `glob.is_match("**/*.rs", "src/.cache/foo.rs")` → `false`
      (the `.cache` segment hides the path)
    - `glob.is_match("**/.cache/*.rs", "src/.cache/foo.rs")` → `true`
]=]
export type Glob = {
    --[=[
        Test whether `path` matches this pattern. Forward slashes and
        backslashes in `path` are normalised before matching. See the
        dotfile rule documented above.
    ]=]
    is_match: (self: Glob, path: string) -> boolean,

    --[=[
        Walk the filesystem and return all paths matching this pattern.

        Returns `(results, nil)` on a clean walk; `(results, errors)`
        where `errors` is an array of `{path, error}` records for
        entries the walk could not descend into; `(nil, message)` on
        hard failure (cwd missing, invalid option).
    ]=]
    scan: (self: Glob, opts: ScanOpts?) -> ({ string }?, (string | { ScanError })?),
}

--[=[
    Compiled set of glob patterns — multiple patterns into one
    automaton. Construct with `glob.GlobSet.new(patterns)`.
]=]
export type GlobSet = {
    --[=[
        True if any pattern in the set matches `path`. Short-circuits
        on the first match.
    ]=]
    is_match: (self: GlobSet, path: string) -> boolean,

    --[=[
        1-based indices of every pattern in the set that matches
        `path`. Returns all matches (no short-circuit). Empty table
        when nothing matches.
    ]=]
    matches: (self: GlobSet, path: string) -> { number },

    --[=[
        Number of patterns in the set.
    ]=]
    len: (self: GlobSet) -> number,
}

--[=[
    Sub-table holding the `Glob` constructor.
]=]
export type GlobConstructor = {
    --[=[
        Compile `pattern`. Returns `(Glob, nil)` on success or
        `(nil, "glob.Glob.new: ...")` on a pattern error.
    ]=]
    new: (pattern: string) -> (Glob?, string?),
}

--[=[
    Sub-table holding the `GlobSet` constructor.
]=]
export type GlobSetConstructor = {
    --[=[
        Compile `patterns` into a single set. Rejects the entire set
        if any individual pattern is invalid (no silent drops).
        `new({})` succeeds and matches nothing.
    ]=]
    new: (patterns: { string }) -> (GlobSet?, string?),
}

--[=[
    The module table returned by `require("lib:glob")`.
]=]
export type GlobModule = {
    Glob: GlobConstructor,
    GlobSet: GlobSetConstructor,

    --[=[
        Compile-and-match in one call. Returns `(boolean, nil)` on
        success or `(nil, "glob.is_match: ...")` on a pattern error.

        ```lua
        local glob = require("lib:glob")
        local ok = glob.is_match("src/**/*.rs", "src/lua/fs.rs")
        ```
    ]=]
    is_match: (pattern: string, path: string) -> (boolean?, string?),

    --[=[
        Compile-and-scan in one call. See `Glob:scan` for the return
        contract.

        ```lua
        local files = glob.scan("src/**/*.rs", {
            cwd    = ".",
            ignore = { "**/target/**" },
        })
        ```
    ]=]
    scan: (pattern: string, opts: ScanOpts?) -> ({ string }?, (string | { ScanError })?),
}

declare function require(name: "lib:glob"): GlobModule
