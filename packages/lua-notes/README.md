# lua-notes

A notes app on **plain Lua 5.1** (the `lua` CLI), same **ports & adapters** model
as the http-sqlite examples — fitted to the runtime. The neoc twin of
`luau-notes`, targeting `lua` instead of `luau`.

Plain Lua has no HTTP server and no database, so this is a console app and the
storage port is an **in-memory `Store`** instead of SQL over HTTP. The shape is
identical: neoc owns the controllers + the port; the Lua host implements the
port and drives.

## How it's wired

```
        implements the Store port              calls; gets strings back
  ┌──────────────────────────────┐      ┌──────────────────────────────┐
  │  host  app.lua  (plain Lua)   │ ───▶ │  neoc controllers  main.lua  │
  │  • Store = in-memory table    │      │  create / list               │
  │    (add / count / at)         │ ◀─── │  (generic over Store)        │
  │  • drives + print()s          │      │  call store:add / :count/:at │
  └──────────────────────────────┘      └──────────────────────────────┘
```

1. **`src/main/neoc/main.neoc`** — the controllers, in pure neoc: a `Store`
   *port* (a trait — `add` / `count` / `at`, primitives only) and `create` /
   `list`, generic over `Store`. `create` carries the business rule (reject an
   empty body). No host concern; the port speaks only `string`/`number`, so the
   host never constructs a neoc value. Source is identical to `luau-notes` — the
   controllers and port are target-agnostic.
2. **`neoc build`** transpiles the project (target `lua`) to `src/auto/lua/`:
   - `main.lua` — a requireable module that ends with
     `return { create = create, list = list }`.
   - `main.d.lua` — **LuaLS typings** for the generated module (`---@param` /
     `---@return`, `function M.create(...)`, `return M`). Point your editor's
     Lua language server at this and the plain-Lua host gets full completion and
     type checks on `app.create` / `app.list` despite `main.lua` being untyped.
   - `main.lua.map` — source map back to the `.neoc`.
   - `neoc-lib.d.lua` — shared runtime typings.
3. **`src/main/lua/app.lua`** — the host, plain Lua 5.1. Plain Lua `require`
   searches `package.path`, not relative-to-file, so it sets `package.path` from
   the script's own location, then `require("main")`. It implements `Store` as an
   in-memory Lua table (neoc's `store.add(b)` lowers to the colon call
   `store:add(b)`, so the methods take `self`), drives a short sequence, and
   `print`s the results.

The direction is the point: the host **requires and calls** neoc; neoc **calls
back** into the `Store` port the host implements. Same model as
`bun-http-sqlite` / `python-http-sqlite`, on a runtime with no I/O.

## Run

```sh
bun run build    # neoc build (emits src/auto/lua/)
bun run start    # lua src/main/lua/app.lua
```

```
added: hello
added: world
error: body must not be empty
hello | world |
```
