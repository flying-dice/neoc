--!strict
--
-- Type definitions for `lib:json` — paired with `src/lua/lib/json.rs`.
-- Co-located with the Rust source so any change to the Lua-facing
-- surface is reviewed alongside the implementation, and so a reader
-- landing on the .rs file sees its types one directory entry away.
--
-- ## How this file is consumed
--
-- The luau-lsp language server reads it via the
-- `luau-lsp.types.definitionFiles` editor setting and applies the
-- declarations globally during type-check / hover / autocomplete on
-- regular `.luau` files. The literal-string-overloaded `require`
-- declaration at the bottom is what makes
-- `local json = require("lib:json")` come back as a typed `JsonModule`
-- rather than `any`.
--
-- ## Editor diagnostics on this file are expected to be noisy
--
-- This is a `.d.luau` *definition file*, which has its own dialect of
-- Luau (the `declare` keyword family). Editors and the regular Luau
-- type-checker parse it as a normal script and will flag `declare`
-- statements as syntax errors — that's a tooling-level mismatch, not
-- a problem with the file. luau-lsp consumes it correctly. **Don't
-- "fix" the squiggles by removing `declare` lines — they're load-
-- bearing and the file is functionally correct as-is.**

--[=[
    A JSON value as it can appear at any node in the tree.

    The two userdata types (`JsonObject`, `JsonArray`) keep their data
    Rust-side and are addressed through methods. Scalar leaves
    (`string` / `number` / `boolean`) materialise as Lua-native values
    on access.

    JSON `null` comes back as the `json.null` sentinel — a userdata
    distinct from Lua `nil`. Lua nil here means "key absent" or "path
    missed"; `json.null` means "key present, value was null". The
    sentinel's type is `any` in this declaration only because Luau has
    no first-class `userdata` keyword; the runtime type is a unique
    opaque userdata with `__eq` defined so any returned `null` compares
    equal to `json.null`.

    ```lua
    local json = require("lib:json")

    body:get("missing_key")  == nil        -- true: key wasn't there
    body:get("explicit_null") == json.null -- true: key present, value was null
    body:has("missing_key")                -- false
    body:has("explicit_null")              -- true
    ```
]=]
export type JsonValue =
    JsonObject
    | JsonArray
    | string
    | number
    | boolean
    | nil
    | any -- the `json.null` sentinel; see comment above

--[=[
    Userdata handle for a JSON object node.

    Internally, every handle (Object, Array, intermediate or root) holds
    a reference-counted pointer to the parsed document plus a path that
    locates this node within it. Access methods walk the path under a
    single read or write lock; no data leaves the Rust side until you
    ask for a scalar or call `:to_table()`.

    There is **no** `obj.field` sugar by design. JSON shape is dynamic
    and the type system can't know what fields exist or what types they
    have, so field-access syntax would silently mask type errors. Every
    access goes through `:get(...)` so the dynamic boundary is explicit
    at every call site.
]=]
export type JsonObject = {
    --[=[
        Walk a path of segments and return the leaf node.

        - For object children, the result is a `JsonObject` userdata
          referencing the same document at a deeper path.
        - For array children, a `JsonArray` userdata.
        - For scalar leaves (string/number/boolean), a Lua-native
          value materialised on access.
        - For JSON `null` or any segment that fails to resolve, `nil`.

        The first segment must be a string (object key from this
        object). Subsequent segments can be either:
        - `string` — into a nested object
        - `number` (positive integer) — into a nested array, 1-based

        Multi-hop calls go under a **single** lock acquire — the whole
        path is resolved in one pass. Chaining `:get()`-per-hop also
        works but takes one lock per hop, so prefer the variadic form
        for deep navigation.

        ```lua
        body:get("name")                    -- "Alice"
        body:get("users", 1, "email")       -- one lock, three hops
        body:get("users"):get(1):get("email") -- three locks, three hops
        body:get("missing")                 -- nil
        ```
    ]=]
    get: (self: JsonObject, ...any) -> JsonValue,

    --[=[
        Insert or replace `key` with `value`.

        The value may be a Lua native (string/number/boolean/nil), a
        Lua table (deep-converted: detected as JSON array if its keys
        are 1..N, otherwise object), or another `JsonObject` /
        `JsonArray` handle (whose underlying subtree is cloned into
        place — the inserted value is independent of the source).

        Mutation is observed by every other handle that points at this
        node or below, because they share the same underlying document.

        Acquires the document's write lock for the duration of the
        insert.
    ]=]
set: (self: JsonObject, key: string, value: any) -> (),

    --[=[
        Returns `true` if the object contains an entry for `key`,
        regardless of whether the value is `null`.

        Use this rather than `obj:get(k) == nil` when you need to
        distinguish "key absent" from "key present with null value".
    ]=]
    has: (self: JsonObject, key: string) -> boolean,

    --[=[
        Returns the object's keys as a 1-indexed Lua table of strings.

        Order is alphabetical because the underlying storage is a
        sorted map; this means encoded output and key iteration are
        deterministic across runs and machines.
    ]=]
    keys: (self: JsonObject) -> {string},

    --[=[
        Number of top-level keys in this object.

        Note: this is the count of immediate keys, not a recursive
        size. For an object containing one key whose value is a 100-
        element array, `:len()` returns 1.
    ]=]
    len: (self: JsonObject) -> number,

    --[=[
        Materialise the entire subtree under this node as a Lua table.

        This is the GC-heavy operation the userdata model normally
        avoids — every key, every leaf, every nested table is allocated
        in the Lua heap. Call it only when you actually need a plain
        Lua table (for example, to pass to a library that doesn't know
        about JsonObject userdata).

        The conversion is one-shot: subsequent mutations on the
        userdata don't propagate to the returned table, and vice
        versa.
    ]=]
    to_table: (self: JsonObject) -> {[string]: any},
}

--[=[
    Userdata handle for a JSON array node.

    Same architectural shape as `JsonObject`: a reference-counted
    pointer to the parsed document plus a path. Indices are 1-based to
    match Lua convention.
]=]
export type JsonArray = {
    --[=[
        Walk a path starting from a 1-based index into this array.

        The first segment must be a positive integer. Subsequent
        segments can be `string` (into a nested object) or `number`
        (into a nested array). Returns the same union as
        `JsonObject:get` — handle, scalar, or `nil`.

        ```lua
        users:get(1)                -- first user (a JsonObject)
        users:get(1, "name")        -- first user's name (a string)
        users:get(99)               -- nil (out of bounds)
        ```
    ]=]
    get: (self: JsonArray, ...any) -> JsonValue,

    --[=[
        Replace the element at `index` (1-based).

        - Errors if `index < 1`.
        - If `index > #self`, the array is grown with `null` fill so
          the new element lands at the requested position.

        Same value-coercion rules as `JsonObject:set` — Lua native,
        Lua table, or another userdata handle are all accepted.
    ]=]
    set: (self: JsonArray, index: number, value: any) -> (),

    --[=[
        Append `value` to the end of the array. Equivalent to
        `arr:set(arr:len() + 1, value)` but in one call.
    ]=]
    push: (self: JsonArray, value: any) -> (),

    --[=[
        Current length of the array.
    ]=]
    len: (self: JsonArray) -> number,

    --[=[
        Materialise the entire array (and any nested objects/arrays
        below) as a 1-indexed Lua table. Same caveat as
        `JsonObject:to_table` — pays the full GC cost; the result is
        a snapshot, not a view.
    ]=]
    to_table: (self: JsonArray) -> {any},
}

--[=[
    The module table returned by `require("lib:json")`.

    `lib:json` is the in-memory data structure layer. Parsing JSON
    text and serialising back lives in `vnd:serde_json` — start there
    if you have bytes; come here if you want to construct documents
    programmatically or inspect/mutate already-parsed handles.
]=]
export type JsonModule = {
    --[=[
        Construct an empty mutable JSON object.

        Returns a fresh `JsonObject` rooted in its own document — not
        connected to any other handle. Pass it to `:set` or `:push`
        on another handle to graft it in.
    ]=]
    object: () -> JsonObject,

    --[=[
        Construct an empty mutable JSON array. Same lifecycle as
        `object` — fresh document, ready for `:push` or `:set`.
    ]=]
    array: () -> JsonArray,

    --[=[
        Sentinel userdata for explicit JSON `null`.

        `:get` returns this when the resolved JSON value is `null`, as
        opposed to Lua `nil` which means the path didn't resolve at
        all. Compare against this to tell the two apart:

        ```lua
        local json = require("lib:json")

        local v = body:get("maybe_null_field")
        if v == json.null then
            -- present, value is null
        elseif v == nil then
            -- key absent / path missed
        else
            -- v has a real value
        end
        ```

        Type is `any` here only because Luau has no `userdata` type
        keyword; the runtime value is a unique opaque userdata with
        `__eq` so all references to `json.null` compare equal.
    ]=]
    null: any,
}

-- Literal-string overload — picked up by luau-lsp so a call site
-- like `local json = require("lib:json")` resolves `json` to
-- `JsonModule` rather than `any`.
declare function require(name: "lib:json"): JsonModule