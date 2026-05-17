--!strict
--
-- Type definitions for `std:env` — paired with `src/lua/std/env.rs`.

--[=[
    The module table returned by `require("std:env")`.

    Exposes platform constants, directory path queries, read-only environment
    variable access, and CLI argument access. Mirrors `std::env` from Rust.
]=]
export type EnvModule = {
    os: string,
    arch: string,
    family: string,
    exe_suffix: string,
    dll_prefix: string,
    dll_suffix: string,

    home_dir: () -> (string?, string?),
    temp_dir: () -> string,
    current_dir: () -> (string?, string?),

    var: (key: string) -> (string?, string?),
    vars: () -> { [string]: string },

    args: () -> { string },
}

declare function require(name: "std:env"): EnvModule
