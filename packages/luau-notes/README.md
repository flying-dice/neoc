# luau-notes

A notes app on **raw Luau** (the `luau` CLI), same **ports & adapters** model as
the http-sqlite examples — fitted to the runtime.

Raw Luau has no HTTP server and no database, so this is a console app and the
storage port is an **in-memory `Store`** instead of SQL over HTTP. The shape is
identical: neoc owns the controllers + the port; the Luau host implements the
port and drives.

## How it's wired

```
        implements the Store port              calls; gets strings back
  ┌──────────────────────────────┐      ┌──────────────────────────────┐
  │  host  app.luau  (raw Luau)   │ ───▶ │  neoc controllers  main.luau │
  │  • Store = in-memory table    │      │  create / list               │
  │    (add / count / at)         │ ◀─── │  (generic over Store)        │
  │  • drives + print()s          │      │  call store:add / :count/:at │
  └──────────────────────────────┘      └──────────────────────────────┘
```

1. **`src/main/neoc/main.neoc`** — the controllers, in pure neoc: a `Store`
   *port* (a trait — `add` / `count` / `at`, primitives only) and `create` /
   `list`, generic over `Store`. `create` carries the business rule (reject an
   empty body). No host concern; the port speaks only `string`/`number`, so the
   host never constructs a neoc value.
2. **`neoc build`** transpiles the project (target `luau`) to `out/main.luau` —
   a requireable module that ends with `return { create = create, list = list }`.
3. **`src/main/host/app.luau`** — the host. It implements `Store` as an in-memory
   Luau table (neoc's `store.add(b)` lowers to the colon call `store:add(b)`, so
   the methods take `self`), `require`s the controllers, drives a short sequence,
   and `print`s the results.

The direction is the point: the host **requires and calls** neoc; neoc **calls
back** into the `Store` port the host implements. Same model as
`bun-http-sqlite` / `python-http-sqlite`, on a runtime with no I/O.

## Run

```sh
bun run build    # neoc build (emits out/main.luau)
bun run start    # luau src/main/host/app.luau
```

```
added: hello
added: world
error: body must not be empty
hello | world |
```
