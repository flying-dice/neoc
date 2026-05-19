--!strict
--
-- Type definitions for `lib:args` — paired with `src/lua/lib/args.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    Flag type discriminator. Booleans are presence-only (clap default —
    `--flag` sets true, absent is false; `--flag=true`/`--flag=false` are
    rejected). Strings and numbers take a value via `--flag value` or
    `--flag=value`.
]=]
export type ArgsFlagType = "boolean" | "string" | "number"

--[=[
    Specification for a single flag.

    Either declared as a map entry `{ verbose = { type = "boolean", ... } }`
    or as an array entry with an explicit `name` field — the array form
    preserves definition order in help output, the map form sorts
    alphabetically (Luau pair-iteration order is implementation-defined).
]=]
export type ArgsFlagSpec = {
    name: string?,
    short: string?,
    type: ArgsFlagType,
    default: (boolean | string | number)?,
    required: boolean?,
    multiple: boolean?,
    desc: string?,
}

--[=[
    Per-subcommand spec — same shape as the top-level command minus a
    requirement on `name` (subcommand name comes from the map key).
]=]
export type ArgsSubcommandSpec = {
    description: string?,
    flags: ({ ArgsFlagSpec } | { [string]: ArgsFlagSpec })?,
}

--[=[
    Top-level parser config.

    `name` is required and shows up in help output. `flags` accepts either
    an ordered array of entries (each with `name = "..."`) or a map keyed
    on flag name. `subcommands` is single-depth; nested subcommands are
    rejected at config-validation time.
]=]
export type ArgsConfig = {
    name: string,
    description: string?,
    flags: ({ ArgsFlagSpec } | { [string]: ArgsFlagSpec })?,
    subcommands: { [string]: ArgsSubcommandSpec }?,
}

--[=[
    The parsed result table.

    Flag values appear under their long names — `parsed.verbose`,
    `parsed.output`. `multiple = true` flags are returned as arrays.
    `positional` is everything that wasn't a flag; `rest` is everything
    after the first `--` separator (always a table — empty when absent).

    On `--help` / `-h`: `help = true` and `help_text` is set with clap's
    rendered help; required-flag validation is skipped so the user can ask
    for help without satisfying constraints first. Errors and help text
    track clap's formatter — the wording is not API-stable across clap
    minor version bumps.

    With subcommands: `parsed.subcommand` names the matched subcommand and
    `parsed[name]` is its scoped result table (same shape).
]=]
export type ArgsParsed = {
    [string]: any,
    positional: { string },
    rest: { string },
    subcommand: string?,
    help: boolean?,
    help_text: string?,
}

export type ArgsModule = {
    parse: ((argv: { string }, config: ArgsConfig) -> (ArgsParsed, nil)) & ((argv: { string }, config: ArgsConfig) -> (nil, string)),
    help: ((config: ArgsConfig) -> (string, nil)) & ((config: ArgsConfig) -> (nil, string)),
}

declare function require(name: "lib:args"): ArgsModule

return nil :: any
