--!strict
--
-- Type definitions for `lib:semver` — paired with `src/lua/lib/semver.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Increment level for `Version:increment` and `semver.increment`.

    - `"major"` — bump major, reset minor/patch, drop pre and build
    - `"minor"` — bump minor, reset patch, drop pre and build
    - `"patch"` — bump patch (drops a pre-release without bumping per
      SemVer 2.0.0 §11), drop build
    - `"prerelease"` — increment the trailing numeric identifier of the
      pre-release; on a non-numeric tail (`1.2.3-beta`), append `.0`
      (`1.2.3-beta.0`). When the input has no pre, increments patch and
      adds `-0`. Build metadata is stripped from every result.
]=]
export type IncLevel = "major" | "minor" | "patch" | "prerelease"

--[=[
    Parsed semver version. Construct with `semver.Version.parse(s)` or the
    flat alias `semver.parse(s)`. Immutable — every operation returns a new
    value. `tostring(v)` returns the canonical SemVer 2.0.0 string.

    ```lua
    local v = semver.Version.parse("1.2.3-beta.1+build.42")
    v.major   -- 1
    v.minor   -- 2
    v.patch   -- 3
    v.pre     -- "beta.1"
    v.build   -- "build.42"
    ```

    Comparison metamethods (`<`, `<=`, `==`) are spec-correct and enable
    `table.sort(versions)` without a custom comparator.
]=]
export type Version = {
    --[=[ Major version component. ]=]
    major: number,
    --[=[ Minor version component. ]=]
    minor: number,
    --[=[ Patch version component. ]=]
    patch: number,
    --[=[ Pre-release identifier (dot-joined) or `nil` if absent. ]=]
    pre: string?,
    --[=[ Build metadata (dot-joined) or `nil` if absent. Ignored in
        precedence per SemVer 2.0.0 §10. ]=]
    build: string?,

    --[=[
        Three-way compare. Returns `-1`, `0`, or `1`. Accepts another
        `Version` or a string (parsed on demand; raises if invalid).
    ]=]
    compare: (self: Version, other: Version | string) -> number,

    --[=[
        Return a new `Version` incremented per `level`. The input is not
        mutated. See `IncLevel` for the rules.
    ]=]
    increment: (self: Version, level: IncLevel) -> Version,
}

--[=[
    Parsed version requirement (range). Construct with
    `semver.VersionReq.parse(s)`. Range syntax is **Cargo-compatible**:
    `^`, `~`, `>=`, `<`, `=`, comma-AND. npm extensions (`||`, hyphen
    ranges, x-ranges) are out of scope and will fail to parse.
    `tostring(r)` returns the canonical Cargo range form.

    ```lua
    local r = semver.VersionReq.parse(">=1.0.0, <2.0.0")
    r:matches("1.5.0")    -- true
    r:matches("2.0.0")    -- false
    tostring(r)           -- ">=1.0.0, <2.0.0"
    ```
]=]
export type VersionReq = {
    --[=[
        True when `version` is in the range. Accepts a `Version` or a
        string. Raises if a string is not a valid version.
    ]=]
    matches: (self: VersionReq, version: Version | string) -> boolean,
}

--[=[
    Sub-table holding the `Version` constructor.
]=]
export type VersionConstructor = {
    --[=[
        Parse a SemVer 2.0.0 string. Returns `(Version, nil)` on success or
        `(nil, "semver.Version.parse: ...")` if the string is malformed.
    ]=]
    parse: (s: string) -> (Version?, string?),
}

--[=[
    Sub-table holding the `VersionReq` constructor.
]=]
export type VersionReqConstructor = {
    --[=[
        Parse a Cargo-compatible range string. Returns `(VersionReq, nil)`
        on success or `(nil, "semver.VersionReq.parse: ...")` if the
        grammar is rejected.
    ]=]
    parse: (s: string) -> (VersionReq?, string?),
}

--[=[
    The module table returned by `require("lib:semver")`.
]=]
export type SemverModule = {
    Version: VersionConstructor,
    VersionReq: VersionReqConstructor,

    --[=[
        Alias for `Version.parse`. Returns `(Version, nil)` on success or
        `(nil, "semver.parse: ...")` on malformed input.
    ]=]
    parse: (s: string) -> (Version?, string?),

    --[=[
        True when `s` parses as a strict SemVer 2.0.0 version. Never
        errors — this is the predicate form of `parse`.
    ]=]
    valid: (s: string) -> boolean,

    --[=[
        Loose extraction. Strips a leading `v`/`V`, finds the first numeric
        sequence, fills missing minor/patch with `0`, preserves a trailing
        pre-release identifier (or drops it if it violates SemVer 2.0.0 §9 —
        e.g. leading-zero numeric segments), drops build metadata, truncates
        fourth components (`1.2.3.4` → `1.2.3`).

        Returns `(Version, nil)` on success or `(nil, "semver.coerce: ...")`
        when no numeric sequence can be extracted.
    ]=]
    coerce: (s: string) -> (Version?, string?),

    --[=[
        Three-way compare. Returns `-1`, `0`, or `1`. Both args may be a
        `Version` or a string; mixed inputs produce identical results to
        comparing parsed values.
    ]=]
    compare: (a: Version | string, b: Version | string) -> (number?, string?),

    --[=[
        True when `version` is in `range`.

        Returns `false` when the range is well-formed but the version
        does not match. Returns `(nil, "semver.satisfies: ...")` only on
        a malformed range or version string — never on a clean "no
        match" result.

        ```lua
        semver.satisfies("1.2.3", "^1.0.0")             -- true
        semver.satisfies("2.0.0", "^1.0.0")             -- false
        semver.satisfies("1.0.0-alpha", "^1.0.0")       -- false (pre)
        ```
    ]=]
    satisfies: (
        version: Version | string,
        range: VersionReq | string
    ) -> (boolean?, string?),

    --[=[
        Increment `version` by `level` and return a SemVer 2.0.0 string.
        Build metadata is stripped from the result. See `IncLevel` for
        the rules.

        Returns `(string, nil)` on success or `(nil, "semver.increment: ...")`
        if either argument is invalid.
    ]=]
    increment: (version: string, level: IncLevel) -> (string?, string?),

    --[=[
        Canonical SemVer 2.0.0 string for `v`. Equivalent to `tostring(v)`.
        Accepts a `Version` or a string (parsed on demand).

        Returns `(string, nil)` on success or `(nil, "semver.format: ...")`
        if the argument is not a valid version.
    ]=]
    format: (v: Version | string) -> (string?, string?),
}

declare function require(name: "lib:semver"): SemverModule
