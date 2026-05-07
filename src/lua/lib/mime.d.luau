--!strict
--
-- Type definitions for `lib:mime` — paired with `src/lua/lib/mime.rs`.
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
-- `local mime = require("lib:mime")` come back as a typed `MimeModule`
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
    Map a file extension to its MIME type.
    Accepts with or without leading dot: `".json"` and `"json"` both work.
    Returns `(mime_string, nil)` on success, `(nil, error_string)` on failure.
]=]

--[=[
    Map a MIME type to its most common file extension (with leading dot).
    Resolution order: user prefs (optional table) → runtime prefs → mime_guess.
    Returns `(extension, nil)` on success, `(nil, error_string)` on failure.
]=]

--[=[
    Detect the MIME type of a filename by its extension(s).
    Handles compound extensions like `"file.tar.gz"`.
    Returns `(mime_string, nil)` on success, `(nil, error_string)` on failure.
]=]

export type MimeModule = {
    from_extension: (ext: string) -> (string?, string?),
    to_extension: (mime_type: string, prefs: { [string]: string }?) -> (string?, string?),
    detect: (filename: string) -> (string?, string?),
}

-- Literal-string overload — picked up by luau-lsp so a call site
-- like `local mime = require("lib:mime")` resolves `mime` to
-- `MimeModule` rather than `any`.
declare function require(name: "lib:mime"): MimeModule
