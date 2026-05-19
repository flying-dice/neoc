--!strict
--
-- Type definitions for `lib:log` — paired with `src/lua/lib/log.rs`.
-- Co-located with the Rust source so any Lua-facing surface change is
-- reviewed alongside the implementation, and so a reader landing on
-- the .rs file sees its types one directory entry away.
--
-- ## How this file is consumed
--
-- The luau-lsp language server reads it via the
-- `luau-lsp.types.definitionFiles` editor setting and applies the
-- declarations globally during type-check / hover / autocomplete on
-- regular `.luau` files. The literal-string-overloaded `require`
-- declaration at the bottom is what makes
-- `local log = require("lib:log")` come back as a typed `LogModule`
-- rather than `any`.
--
-- ## Editor diagnostics on this file are expected to be noisy
--
-- This is a `.d.luau` *definition file*, which has its own dialect of
-- Luau (the `declare` keyword family). Editors and the regular Luau
-- type-checker parse it as a normal script and will flag `declare`
-- statements as syntax errors — that's a tooling-level mismatch, not
-- a problem with the file. luau-lsp consumes it correctly.

--[=[
    Level threshold for a logger instance.

    - `"trace"` / `"debug"` / `"info"` / `"warn"` / `"error"` — every
      call at this level or higher emits a line; lower levels are
      dropped before any formatting work happens.
    - `"off"` — every call is suppressed. Useful for test harnesses or
      a dormant child logger that should only become noisy under a
      specific code path.
]=]
export type LogLevel = "trace" | "debug" | "info" | "warn" | "error" | "off"

--[=[
    Output format for the stderr line.

    - `"json"` (default) — flat JSON object with `level`, `ts`, `msg`,
      then user fields. Designed to be parsed by log shippers, `jq`,
      and LLM tools without unwrapping nested objects.
    - `"text"` — `ts [level] msg key=value ...`, for human debugging.
]=]
export type LogFormat = "json" | "text"

--[=[
    Construction options for `log.new`. Both fields are optional; the
    defaults are `level = "info"` and `format = "json"`.
]=]
export type LogOptions = {
    level: LogLevel?,
    format: LogFormat?,
}

--[=[
    A field payload accompanying a log call. Values may be any Lua
    primitive, a nested table (depth-limited to 3), or unsupported
    types — functions/threads/userdata are replaced with placeholder
    strings (`"<function>"`, `"<userdata>"`) so a log call can never
    crash the program. `nil` values are omitted from the output line.
]=]
export type LogFields = {[string]: any}

--[=[
    Logger handle. Holds an immutable level + format + bound context
    fields. Every method emits one stderr line.

    ## `with`

    `logger:with(fields)` returns a **new** logger that carries the
    merged context — useful for request-scoped logging:

    ```lua
    local reqlog = logger:with({ request_id = "abc", user_id = 42 })
    reqlog:info("query executed", { rows = 100 })
    -- output line includes request_id and user_id automatically
    ```

    The parent logger is unchanged. Chains compose: a child of a child
    inherits both parents' fields.

    ## Level methods

    All five level methods share the same `(msg, fields?)` signature.
    `msg` is a plain string; `fields` is an optional table whose keys
    become top-level JSON keys in the output line. Reserved names
    (`level` / `ts` / `msg`) cannot be shadowed — caller-supplied
    fields with those names are dropped to keep the output schema
    parseable.
]=]
export type Logger = {
    --[=[
        Return a child logger with `fields` merged into the bound
        context. The parent logger is unchanged. Child loggers
        inherit the parent's `level` and `format`; only the bound
        fields differ.
    ]=]
    with: (self: Logger, fields: LogFields) -> Logger,

    --[=[
        Emit a `trace`-level line if the logger's level threshold is
        `trace`. The lowest, most-verbose level.
    ]=]
    trace: (self: Logger, msg: string, fields: LogFields?) -> (),

    --[=[
        Emit a `debug`-level line if the logger's level threshold is
        `debug` or lower.
    ]=]
    debug: (self: Logger, msg: string, fields: LogFields?) -> (),

    --[=[
        Emit an `info`-level line if the logger's level threshold is
        `info` or lower. This is the default level.
    ]=]
    info: (self: Logger, msg: string, fields: LogFields?) -> (),

    --[=[
        Emit a `warn`-level line if the logger's level threshold is
        `warn` or lower.
    ]=]
    warn: (self: Logger, msg: string, fields: LogFields?) -> (),

    --[=[
        Emit an `error`-level line. Highest severity short of `off`.
    ]=]
    error: (self: Logger, msg: string, fields: LogFields?) -> (),
}

--[=[
    The module table returned by `require("lib:log")`.

    Module-level `log.trace` / `log.debug` / `log.info` / `log.warn` /
    `log.error` are convenience wrappers that delegate to a fresh
    default-configured logger on each call (level=info, format=json,
    no bound fields). For request-scoped context or a different level
    threshold, build an explicit logger with `log.new` and use its
    methods.
]=]
export type LogModule = {
    --[=[
        Construct an explicit logger instance. Pass `level` to control
        the threshold and `format` to switch between JSON and text
        output.

        ```lua
        local logger = log.new({ level = "debug", format = "json" })
        ```
    ]=]
    new: (opts: LogOptions?) -> Logger,

    --[=[ Module-level convenience for the lowest level. ]=]
    trace: (msg: string, fields: LogFields?) -> (),
    --[=[ Module-level convenience for debug. ]=]
    debug: (msg: string, fields: LogFields?) -> (),
    --[=[ Module-level convenience for info — the default level. ]=]
    info: (msg: string, fields: LogFields?) -> (),
    --[=[ Module-level convenience for warn. ]=]
    warn: (msg: string, fields: LogFields?) -> (),
    --[=[ Module-level convenience for error. ]=]
    error: (msg: string, fields: LogFields?) -> (),
}

declare function require(name: "lib:log"): LogModule
