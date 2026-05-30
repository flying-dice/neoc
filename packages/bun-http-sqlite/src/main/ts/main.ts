// The application host, in TypeScript — the entry point and the driver.
//
// neoc owns the contracts (the controllers + the `SqlExecutor` port + the
// `Result` / `Option` / `SqlValue` shapes), transpiled to `../../../out/*` with `.d.ts`
// typings. This host does the three things the model asks of it:
//   1. implements the port neoc declares — the `SqlExecutor` adapter over
//      `bun:sqlite` (the only code that touches a database);
//   2. owns HTTP — `Bun.serve` and the `routes` table;
//   3. maps each controller's `HttpResponse` (status + body, both decided in
//      neoc) onto a runtime `Response`. The controllers own *what* the response
//      is; the host owns only *how* to express it.
//
// Note: the host never touches neoc's runtime encoding (`__enum` / `__tag` /
// `_0`). It builds/reads `SqlValue` through the generated `SqlValue.*`
// constructors and `SqlValue.match`; the `Result` handling now lives in the
// controllers, so the host only reads `HttpResponse.{status, body}`.
import { Database, type SQLQueryBindings } from "bun:sqlite";
import { Ok, Err } from "../../../out/neoc-lib.js";
import { SqlValue } from "../../../out/autosql/ports.js";
import type { SqlExecutor } from "../../../out/autosql/ports";
import { init, list, create } from "../../../out/main.js";

// The `SqlExecutor` port contract is neoc-authored — imported, not restated.
// `SqlValue` carries both the type and the constructor namespace (the TS merge).

// --- the adapter: implement the port over bun:sqlite ---

const database = new Database(":memory:");

// SqlValue -> the JS value bun:sqlite binds. `SqlValue.match` does the read; the
// tag never appears here.
function toBound(v: SqlValue): SQLQueryBindings {
  return SqlValue.match<SQLQueryBindings>(v, {
    Int: (n) => n,
    Real: (n) => n,
    Text: (s) => s,
    Bool: (b) => (b ? 1 : 0),
    Null: () => null,
  });
}

// A raw SQLite cell -> SqlValue, via the generated variant constructors.
function toCell(value: unknown): SqlValue {
  if (typeof value === "number") return SqlValue.Real(value);
  if (typeof value === "bigint") return SqlValue.Real(Number(value));
  if (typeof value === "string") return SqlValue.Text(value);
  if (typeof value === "boolean") return SqlValue.Bool(value);
  return SqlValue.Null;
}

const db: SqlExecutor = {
  execute(sql, params) {
    try {
      return Ok(database.run(sql, params.map(toBound)).changes);
    } catch (e) {
      return Err(String(e));
    }
  },
  query(sql, params) {
    try {
      const rows = database.query(sql).values(...params.map(toBound)) as unknown[][];
      return Ok(rows.map((row) => row.map(toCell)));
    } catch (e) {
      return Err(String(e));
    }
  },
};

// --- startup: schema + seed, by calling the neoc controllers ---

init(db);
create(db, "hello");
create(db, "world");

// --- the host owns HTTP and dispatches routes to the neoc controllers ---

Bun.serve({
  port: 38573,
  routes: {
    // /add/<text> — insert a note. The `:text` param arrives URL-decoded. The
    // controller decides the status + body; the host just maps it.
    "/add/:text": (req) => {
      const r = create(db, req.params.text);
      return new Response(r.body, { status: r.status });
    },
    // anything else — list every note.
    "/*": () => {
      const r = list(db);
      return new Response(r.body, { status: r.status });
    },
  },
});

console.log("listening on http://localhost:38573");
