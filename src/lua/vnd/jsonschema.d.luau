--!strict
--
-- Type definitions for `vnd:jsonschema` — paired with
-- `src/lua/vnd/jsonschema.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Usage pattern
--
-- Compile the schema once at module load time, then call `:is_valid()` or
-- `:validate()` per request. `compile` is the expensive step; the two
-- validation methods are fast path vs detailed-errors trade-off.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Result of a full validation run via `schema:validate(value)`.
    Carries a pass/fail flag and a list of all error messages.
]=]
export type ValidationResult = {
    --[=[
        Returns `true` if the value passed validation, `false` otherwise.
    ]=]
    ok: (self: ValidationResult) -> boolean,

    --[=[
        Returns all validation error messages as a 1-indexed Lua table of
        strings. Empty when validation passed.
    ]=]
    errors: (self: ValidationResult) -> {string},
}

--[=[
    A compiled JSON Schema validator. Created by `jsonschema.compile(json)`.
    The schema is parsed and compiled once; each call to `:is_valid` or
    `:validate` re-uses the compiled representation.

    ```lua
    local jsonschema = require("vnd:jsonschema")
    local SCHEMA = jsonschema.compile([[
        { "type": "object", "required": ["name"], "properties": { "name": { "type": "string" } } }
    ]])

    if not SCHEMA:is_valid(req.json) then
        return { status = 400, body = '{"error":"bad request"}' }
    end
    ```
]=]
export type Schema = {
    --[=[
        Fast validation gate. Returns `true` if `value` conforms to the schema,
        `false` otherwise. Short-circuits on the first error — no error messages
        are built. Use this for handler guards where the detailed failure reason
        is not needed.
    ]=]
    is_valid: (self: Schema, value: any) -> boolean,

    --[=[
        Full validation run. Returns a `ValidationResult` that exposes all
        error messages. More expensive than `is_valid` — only use when you need
        to surface the failure reasons to the caller.
    ]=]
    validate: (self: Schema, value: any) -> ValidationResult,
}

--[=[
    The module table returned by `require("vnd:jsonschema")`.
]=]
export type JsonschemaModule = {
    --[=[
        Parse and compile a JSON Schema string. Errors synchronously (not
        returning `(nil, err)`) if the schema itself is invalid.

        Returns `(Schema, nil)` on success; `(nil, "jsonschema.compile: ...")` on
        error (invalid JSON or invalid schema).

        ```lua
        local jsonschema = require("vnd:jsonschema")
        local USER_SCHEMA = jsonschema.compile([[
            {
                "type": "object",
                "required": ["name", "email"],
                "properties": {
                    "name":  { "type": "string" },
                    "email": { "type": "string" }
                }
            }
        ]])
        ```
    ]=]
    compile: (schema_json: string) -> (Schema?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local jsonschema = require("vnd:jsonschema")` resolves to `JsonschemaModule`.
declare function require(name: "vnd:jsonschema"): JsonschemaModule
