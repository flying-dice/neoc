# python-http-sqlite

A notes HTTP server on **ports & adapters**, with the host driving:

- neoc is a typed library — controllers (business logic) plus the `SqlExecutor`
  **port** it depends on. The route controllers return an `HttpResponse` value
  (a status + a ready-to-send body, both decided in neoc). Pure neoc: no HTTP,
  no `sqlite3`, no entry point, no runtime response object.
- The Python host is the application — it **implements the port** (the `sqlite3`
  adapter), **owns HTTP** (`http.server`), and **maps** each controller's
  `HttpResponse` onto an HTTP response.

```
/add/<text>  → create(db, text) -> HttpResponse   (empty body → 400)
/*           → list(db)          -> HttpResponse
```

This is the Python counterpart of `packages/bun-http-sqlite` — the same notes
app, the same neoc (copied verbatim, target-agnostic), a different host.

## The flow

```
            implements the port (DI)            calls; gets an HttpResponse
  ┌─────────────────────────────┐      ┌──────────────────────────────┐
  │  host  server.py  (Python)   │ ───▶ │  neoc controllers  main.py   │
  │  • sqlite3.connect(":memory:")      │  init / list / create        │
  │  • SqlExecutor over sqlite3  │      │  (generic over SqlExecutor)  │
  │  • HTTPServer + handler      │ ◀─── │  return HttpResponse{status, │
  │  • send_response(r.status)…  │      │  body}; call db.execute/query│
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
   `list_notes` / `add_note` DAO. Regenerate with `npm run gen:sql`.
3. **`neoc build`** transpiles the project (rooted at `main.neoc`) to `out/`:
   `out/main.py` + `out/main.pyi` (the controllers), the autosql modules under
   `out/autosql/`, and the shared `neoc_lib.{py,pyi}` carrying the `Result` /
   `Option` / `SqlValue` helpers. The emitted modules use relative imports
   (`from .autosql.ports import …`, `from ..neoc_lib import …`), so `out/` is
   loaded as a package.
4. **`src/main/python/server.py`** — the host. It implements `SqlExecutor` over
   stdlib `sqlite3` (reading/writing `SqlValue` through the emitted `@dataclass`
   variants `Int` / `Real` / `Text` / `Bool` / `Null` and a structural `match` —
   never a tag field), injects it into the controllers, runs `http.server` with a
   `/add/<text>` route plus a catch-all that lists, and maps each controller's
   `HttpResponse` to an HTTP response (`self.send_response(r.status)` then
   `self.wfile.write(r.body.encode())`).

The direction is the point: the host **imports and calls** neoc; neoc **calls
back** into the port the host implements. Nothing reverse-engineers a host type
into neoc.

## Requirements

Python **≥ 3.10** (the host and the emitted controllers use the `match`
statement). The scripts call `python3.14`; substitute any `python3 >= 3.10`.

## Run

```sh
npm run build    # cargo run -p neoc -- build  (emits out/ with .pyi)
npm run start    # python3.14 src/main/python/server.py -> http://localhost:38573
```

```sh
curl localhost:38573/notes          # 1.0, hello | 2.0, world |
curl localhost:38573/add/from-host  # added                (HTTP 200 from HttpResponse)
curl localhost:38573/notes          # 1.0, hello | 2.0, world | 3.0, from-host |
curl localhost:38573/               # catch-all → lists too
curl localhost:38573/add/           # body must not be empty (HTTP 400)
```
