import type { Result, Option } from "../neoc-lib";

export interface ListNotesRow { id: number; body: string; }

import { Ok, Err, Some, None, Option$, Result$ } from "../neoc-lib";

import type { SqlExecutor } from "./ports";
import type { SqlValue } from "./ports";
import { decode_number } from "./ports";
import { decode_string } from "./ports";
export function list_notes<E extends SqlExecutor>(db: E): Result<ListNotesRow[], string> {
  let params: SqlValue[] = [];
  const __try_0 = db.query("SELECT id, body FROM notes ORDER BY id", params);
  if (__try_0.__tag !== "Ok") return __try_0 as unknown as Result<ListNotesRow[], string>;
  let rows = __try_0._0;
  let out: ListNotesRow[] = [];
  let next = 0;
  for (const row of rows) {
    const __try_1 = decode_list_notes(row);
    if (__try_1.__tag !== "Ok") return __try_1 as unknown as Result<ListNotesRow[], string>;
    let decoded = __try_1._0;
    out[next]! = decoded;
    next = (next + 1);
  }
  return Ok(out);
}
function decode_list_notes(cols: SqlValue[]): Result<ListNotesRow, string> {
  const __try_2 = decode_number(cols[0]!, "list_notes: column 0 (id)");
  if (__try_2.__tag !== "Ok") return __try_2 as unknown as Result<ListNotesRow, string>;
  let id = __try_2._0;
  const __try_3 = decode_string(cols[1]!, "list_notes: column 1 (body)");
  if (__try_3.__tag !== "Ok") return __try_3 as unknown as Result<ListNotesRow, string>;
  let body = __try_3._0;
  return Ok({ __struct: "ListNotesRow", id: id, body: body });
}
