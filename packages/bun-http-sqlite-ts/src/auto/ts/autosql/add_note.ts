import type { Result, Option } from "../neoc-lib";

import type { SqlExecutor } from "./ports";
import type { SqlValue } from "./ports";
export function add_note<E extends SqlExecutor>(db: E, body: string): Result<number, string> {
  let params: SqlValue[] = [{ __enum: "SqlValue", __tag: "Text", _0: body }];
  return db.execute("INSERT INTO notes (body) VALUES (?)", params);
}
