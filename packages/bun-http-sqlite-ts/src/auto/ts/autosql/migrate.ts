import type { Result, Option } from "../neoc-lib";

import { Ok, Err, Some, None, Option$, Result$ } from "../neoc-lib";

import type { SqlExecutor } from "./ports";
export function migrate<E extends SqlExecutor>(db: E): Result<number, string> {
  let applied = 0;
  const __try_0 = db.execute("CREATE TABLE notes ( id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL )", []);
  if (__try_0.__tag !== "Ok") return __try_0 as unknown as Result<number, string>;
  __try_0._0;
  applied = (applied + 1);
  return Ok(applied);
}
