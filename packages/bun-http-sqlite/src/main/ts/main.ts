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
import { Database } from "bun:sqlite";
import { Ok, Err } from "../js/neoc-lib.js";
import type { Result } from "../js/neoc-lib";
import type { SqlValue } from "../js/autosql/ports";
import type { ListNotesRow } from "../js/autosql/list_notes";
import { init, list, create } from "../js/main.js";

// The port contract neoc declares (its `SqlExecutor` trait), restated for the
// host against neoc's emitted `SqlValue` / `Result` types. (neoc does not yet
// project a trait to a `.d.ts` interface; once it does, import it directly.)
interface SqlExecutor {
  execute(sql: string, params: SqlValue[]): Result<number, string>;
  query(sql: string, params: SqlValue[]): Result<SqlValue[][], string>;
}

// --- the adapter: implement the port over bun:sqlite ---

const database = new Database(":memory:");

// SqlValue (a neoc enum) -> the JS value bun:sqlite binds. The runtime shape is
// the tagged object neoc emits, e.g. `{ __enum: "SqlValue", __tag: "Int", _0: 1 }`.
function toBound(v: SqlValue): number | string | null {
  switch (v.__tag) {
    case "Int":
    case "Real":
    case "Text":
      return v._0;
    case "Bool":
      return v._0 ? 1 : 0;
    case "Null":
      return null;
  }
}

// A raw SQLite cell -> SqlValue, so the result is `match`-able back in neoc.
function toCell(value: unknown): SqlValue {
  if (typeof value === "number") return { __enum: "SqlValue", __tag: "Real", _0: value };
  if (typeof value === "bigint") return { __enum: "SqlValue", __tag: "Real", _0: Number(value) };
  if (typeof value === "string") return { __enum: "SqlValue", __tag: "Text", _0: value };
  if (typeof value === "boolean") return { __enum: "SqlValue", __tag: "Bool", _0: value };
  return { __enum: "SqlValue", __tag: "Null" };
}

const db: SqlExecutor = {
  execute(sql, params) {
    try {
      return Ok(database.run(sql, ...params.map(toBound)).changes);
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
  fetch(req: Request): Response {
    const path = new URL(req.url).pathname;
    if (path.startsWith("/add/")) {
      const added = create(db, decodeURIComponent(path.slice("/add/".length)));
      return new Response(added.__tag === "Ok" ? `added` : `error: ${added._0}`);
    }
    const notes = list(db);
    if (notes.__tag === "Err") return new Response(`error: ${notes._0}`);
    const body = notes._0.map((n: ListNotesRow) => `${n.id}:${n.body}`).join(" | ");
    return new Response(notes._0.length ? `${body} | ` : body);
  },
});

console.log("listening on http://localhost:38573");
