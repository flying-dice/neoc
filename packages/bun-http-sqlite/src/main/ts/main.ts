// The application host, in TypeScript — the entry point and the driver.
//
// neoc owns the contracts (the controllers + the `SqlExecutor` port + the
// `Result` / `Option` / `SqlValue` shapes), transpiled to `../js/*` with `.d.ts`
// typings. This host does the three things the model asks of it:
//   1. implements the port neoc declares — the `SqlExecutor` adapter over
//      `bun:sqlite` (the only code that touches a database);
//   2. owns HTTP — `Bun.serve` and the `fetch` handler;
//   3. dispatches each route to a neoc controller (`init` / `list` / `create`),
//      imported from the transpiled neoc and typed via its `.d.ts`.
//
// Note: the host never touches neoc's runtime encoding (`__enum` / `__tag` /
// `_0`). It builds and reads `SqlValue` through the generated `SqlValue.*`
// constructors and `SqlValue.match`, and reads a `Result` through its methods
// (`is_ok` / `unwrap` / `unwrap_err`) — all from neoc's emitted `.d.ts`.
import { Database, type SQLQueryBindings } from "bun:sqlite";
import { Ok, Err } from "../js/neoc-lib.js";
import { SqlValue } from "../js/autosql/ports.js";
import type { SqlValue as SqlValueT } from "../js/autosql/ports";
import type { Result } from "../js/neoc-lib";
import type { ListNotesRow } from "../js/autosql/list_notes";
import { init, list, create } from "../js/main.js";

// The port contract neoc declares (its `SqlExecutor` trait), restated for the
// host against neoc's emitted `SqlValue` / `Result` types. (neoc does not yet
// project a trait to a `.d.ts` interface; once it does, import it directly.)
interface SqlExecutor {
  execute(sql: string, params: SqlValueT[]): Result<number, string>;
  query(sql: string, params: SqlValueT[]): Result<SqlValueT[][], string>;
}

// --- the adapter: implement the port over bun:sqlite ---

const database = new Database(":memory:");

// SqlValue -> the JS value bun:sqlite binds. `SqlValue.match` does the read; the
// tag never appears here.
function toBound(v: SqlValueT): SQLQueryBindings {
  return SqlValue.match<SQLQueryBindings>(v, {
    Int: (n) => n,
    Real: (n) => n,
    Text: (s) => s,
    Bool: (b) => (b ? 1 : 0),
    Null: () => null,
  });
}

// A raw SQLite cell -> SqlValue, via the generated variant constructors.
function toCell(value: unknown): SqlValueT {
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
    // /add/<text> — insert a note. The `:text` param arrives URL-decoded.
    "/add/:text": (req) => {
      const added = create(db, req.params.text);
      return new Response(added.is_ok() ? `added` : `error: ${added.unwrap_err()}`);
    },
    // anything else — list every note.
    "/*": () => {
      const notes = list(db);
      if (notes.is_err()) return new Response(`error: ${notes.unwrap_err()}`);
      const rows = notes.unwrap();
      const body = rows.map((n: ListNotesRow) => `${n.id}:${n.body}`).join(" | ");
      return new Response(rows.length ? `${body} | ` : body);
    },
  },
});

console.log("listening on http://localhost:38573");
