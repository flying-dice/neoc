# bun-http-sqlite

A notes HTTP server on **ports & adapters**, with the host driving:

- neoc is a typed library — controllers (business logic) plus the `SqlExecutor`
  **port** it depends on. The route controllers return an `HttpResponse` value
  (a status + a ready-to-send body, both decided in neoc). Pure neoc: no HTTP,
  no `bun:sqlite`, no entry point, no runtime `Response`.
- The TypeScript host is the application — it **implements the port** (the
  `bun:sqlite` adapter), **owns HTTP** (`Bun.serve`), and **maps** each
  controller's `HttpResponse` onto a runtime `Response`.

```
/add/:text   → create(db, text) -> HttpResponse   (Bun routes; empty body → 400)
/*           → list(db)          -> HttpResponse
```

## The flow

```
            implements the port (DI)            calls; gets an HttpResponse
  ┌─────────────────────────────┐      ┌──────────────────────────────┐
  │  host  main.ts  (TypeScript) │ ───▶ │  neoc controllers  main.js   │
  │  • new Database(":memory:")  │      │  init / list / create        │
  │  • SqlExecutor over bun:sqlite      │  (generic over SqlExecutor)  │
  │  • Bun.serve({ routes })     │ ◀─── │  return HttpResponse{status, │
  │  • new Response(r.body,…)    │      │  body}; call db.execute/query│
  └─────────────────────────────┘      └──────────────────────────────┘
        the executor it injected                 the autosql DAO
```

1. **`src/main/neoc/main.neoc`** — the controllers, in pure neoc: `init`, `list`,
   `create`, each generic over `SqlExecutor` (the port). `list`/`create` return
   an `HttpResponse { status, body }` — the `Result`→response mapping, the status
   codes, and the body (a note id folds into the string via a `#[derive(Display)]`
   view, since neoc has no implicit number→string) all live here. No host concern
   appears.
2. **`src/main/neoc/autosql/`** — `neoc autosql` output from `src/main/sql/`: the
   `SqlValue` enum, the `SqlExecutor` trait (the port), and the `migrate` /
   `list_notes` / `add_note` DAO. Regenerate with `bun run gen:sql`.
3. **`neoc build`** transpiles the project (rooted at `main.neoc`) to `src/auto/js/`:
   `src/auto/js/main.js` + `src/auto/js/main.d.ts` (the controllers), the autosql modules, and
   the shared `neoc-lib.{js,d.ts}` carrying the `Result` / `Option` / `SqlValue`
   helpers.
4. **`src/main/ts/main.ts`** — the host. It implements `SqlExecutor` over
   `bun:sqlite` (typed against neoc's emitted `SqlValue` / `Result`), injects it
   into the controllers, runs `Bun.serve` with a `routes` table (the `/add/:text`
   param route + a `/*` catch-all), and maps each controller's `HttpResponse` to
   a runtime `Response` (`new Response(r.body, { status: r.status })`).

The direction is the point: the host **imports and calls** neoc; neoc **calls
back** into the port the host implements. nothing reverse-engineers a host type
into neoc — no `Unknown`, no generated `types.neoc`, no `autots` (retired — see
`tasks/ts-interop-inversion.md`).

## Run

```sh
bun run build    # gen:sql -> neoc build (emits src/auto/js/ with .d.ts)
bun run start    # bun src/main/ts/main.ts -> http://localhost:38573
```

```sh
curl localhost:38573/notes          # 1, hello | 2, world |
curl localhost:38573/add/from-host  # added                (HTTP 200 from HttpResponse)
curl localhost:38573/notes          # 1, hello | 2, world | 3, from-host |
curl localhost:38573/              # /* catch-all → lists too
```
