import { Ok, Err, Some, None, Option$, Result$ } from "../neoc-lib.js";

export function migrate(db) {
  let applied = 0;
  const __try_0 = db.execute("CREATE TABLE notes ( id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL )", []);
  if (__try_0.__tag === "Err" || __try_0.__tag === "None") return __try_0;
  __try_0._0;
  applied = (applied + 1);
  return Ok(applied);
}
