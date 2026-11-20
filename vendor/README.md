# vendor

## mlua

`vendor/mlua` is mlua **0.12.2** (crates.io checksum `7a7ae6b65bf54b7fdd4cf5b6f44f60862350ebf3fe3779b3a9cc005e7fa14f81`, upstream commit
`a186d461c7772aed4cb0a9fa1d1899121c6450e7`, see its `.cargo_vcs_info.json`), copied from the registry with the
`LICENSE` (MIT) kept. Two files are not copied: the registry's `.cargo-ok` marker and `Cargo.lock` (mlua's own
`.gitignore` excludes it, and Cargo ignores a dependency's lockfile). The root `Cargo.toml` applies it with
`[patch.crates-io]`; `mlua-sys` and `mlua_derive` still come from crates.io. It is not a workspace member.

The first commit that added it is the registry source byte for byte; the changes are the patches in
`vendor/mlua-patches/`, applied on top.

### Patches

| Patch | Ref | What |
|---|---|---|
| `0001-isolate-tostring-in-the-luajit-message-handler.patch` | NEOC-124 | LuaJIT only: mlua's message handler (`util::error::error_traceback`) runs the error value's `__tostring` under its own protected call, and refuses a nested conversion on the same Lua state. |

### Why 0001

mlua runs every protected call it makes for Rust (`Table::get`, a conversion, `Function::call`, `call_async`,
`exec`) under a message handler that turns a non-string error value into text with `luaL_tolstring`, which runs
the value's `__tostring`. LuaJIT sets `L->status` to `LUA_ERRERR` while a handler runs, so an error raised inside it
is answered with "error in error handling". A `pcall` inside the handler that catches an error clears that status,
and a later error is then handed to the same handler again. This aborts the process ("stack overflow, aborting"):

```lua
local mt = {}
mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end
-- any host function that reads a table whose __index raises setmetatable({}, mt)
```

PUC Lua 5.1 answered with a catchable error. The patch (`error_traceback` is used on every backend, the fix only on
`feature = "luajit"`) does two things:

1. `__tostring` runs through `lua_pcall` with no message handler, so an error it raises is caught where it is
   converted and never handed back to the handler. The result is the same text as before for a working
   `__tostring`, and `error in error handling` (the string LuaJIT answers natively) for one that raises.
2. A flag in mlua's `ExtraData` is set around that call. The handler entered while it is set (a `__tostring` that fails
   through the Rust API, so that mlua's handler runs nested) answers `error in error handling` without running
   `__tostring`. The flag is per Lua state, not per coroutine: a `__tostring` that creates a coroutine for every
   level would otherwise get a fresh flag each time. A nested conversion that is not an attack loses its text for
   the same reason, which is the price of that.

The flag cannot leak: the protected call returns to the handler in every case (it catches every Lua error), so the
reset after it always runs, and it does not depend on Rust destructors running while Lua unwinds, which mlua does
not promise on every backend.

### Re-applying or offering upstream

```sh
cd <an unpacked mlua 0.12.x>
git apply -p1 /path/to/vendor/mlua-patches/0001-isolate-tostring-in-the-luajit-message-handler.patch
```

The paths in the patch are relative to the mlua root (`src/state/extra.rs`, `src/util/error.rs`). The tests are in
`crates/neoc-vm/tests/mlua_error_handler.rs`. As of 0.12.2 (the latest release) and upstream `main` the handler has
no guard of its own. Remove `[patch.crates-io]`, `vendor/mlua` and this directory when a release carries one.
