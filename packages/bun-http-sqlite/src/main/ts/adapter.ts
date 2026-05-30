// Host adapter for the neoc `db` / `http` / `text` ports declared in
// `../neoc/main.neoc`. neoc owns the contracts — the port signatures and the
// `SqlValue` / `Result` / `Option` shapes; this file *implements* them over
// `bun:sqlite` and `Bun.serve`, typed against neoc's own emitted declarations
// (`../js/neoc-lib`, `../js/autosql/ports`). The neoc→TS projection is total, so
// these types line up exactly with the runtime values neoc passes and expects —
// the boundary is lossless and a `match` on the other side round-trips.
import { Database } from "bun:sqlite";
import { Ok, Err, Some, None } from "../js/neoc-lib.js";
import type { Result, Option } from "../js/neoc-lib";
import type { SqlValue } from "../js/autosql/ports";

// The single bun:sqlite handle, owned entirely by the adapter. The neoc app
// never holds it — it only speaks to the `db` port.
const database = new Database(":memory:");

// SqlValue (a neoc enum) → the JS value bun:sqlite binds. The runtime shape is
// the tagged object neoc emits, e.g. `{ __enum: "SqlValue", __tag: "Int", _0: 1 }`.
function toBound(v: SqlValue): number | string | null {
  switch (v.__tag) {
    case "Int":
    case "Real":
      return v._0;
    case "Text":
      return v._0;
    case "Bool":
      return v._0 ? 1 : 0;
    case "Null":
      return null;
  }
}

// A raw SQLite cell → SqlValue. Building the tagged object directly is what
// makes the value `match`-able back in neoc.
function toCell(value: unknown): SqlValue {
  if (typeof value === "number") return { __enum: "SqlValue", __tag: "Real", _0: value };
  if (typeof value === "bigint") return { __enum: "SqlValue", __tag: "Real", _0: Number(value) };
  if (typeof value === "string") return { __enum: "SqlValue", __tag: "Text", _0: value };
  if (typeof value === "boolean") return { __enum: "SqlValue", __tag: "Bool", _0: value };
  return { __enum: "SqlValue", __tag: "Null" };
}

// --- the `db` port ---

export function execute(sql: string, params: SqlValue[]): Result<number, string> {
  try {
    const info = database.run(sql, ...params.map(toBound));
    return Ok(info.changes);
  } catch (e) {
    return Err(String(e));
  }
}

export function query(sql: string, params: SqlValue[]): Result<SqlValue[][], string> {
  try {
    const rows = database.query(sql).values(...params.map(toBound)) as unknown[][];
    return Ok(rows.map((row) => row.map(toCell)));
  } catch (e) {
    return Err(String(e));
  }
}

// --- the `http` port ---

export function serve(port: number, handler: (path: string) => string): void {
  Bun.serve({
    port,
    fetch(req: Request): Response {
      return new Response(handler(new URL(req.url).pathname));
    },
  });
  console.log(`listening on http://localhost:${port}`);
}

// --- the `text` port ---

export function strip_prefix(s: string, prefix: string): Option<string> {
  return s.startsWith(prefix) ? Some(s.slice(prefix.length)) : None;
}

export function from_number(n: number): string {
  return String(n);
}
