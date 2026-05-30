import type { Result, Option } from "../neoc-lib";

export function add_note<E>(db: E, body: string): Result<number, string> {
  let params = [{ __enum: "SqlValue", __tag: "Text", _0: body }];
  return db.execute("INSERT INTO notes (body) VALUES (?)", params);
}
