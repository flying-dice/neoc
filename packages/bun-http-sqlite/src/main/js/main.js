import * as db from "../ts/adapter.ts";
import * as http from "../ts/adapter.ts";
import * as text from "../ts/adapter.ts";
const __t = {};
import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

import { migrate } from "./autosql/migrate.js";
import { list_notes } from "./autosql/list_notes.js";
import { add_note } from "./autosql/add_note.js";
const Db$ = {};
__t.Db = ["SqlExecutor"];
Db$.execute = function(sql, params) {
  return db.execute(sql, params);
}
Db$.query = function(sql, params) {
  return db.query(sql, params);
}
function render(notes) {
  let out = "";
  for (const n of notes) {
    out = ((((out + text.from_number(n.id)) + ":") + n.body) + " | ");
  }
  return out;
}
function handle(db, path) {
  return (() => {
    const __s1 = text.strip_prefix(path, "/add/");
    if (__s1.__tag === "Some") {
      const body = __s1._0;
      return (() => {
        const __s2 = add_note(db, body);
        if (__s2.__tag === "Ok") {
          return ("added: " + body);
        }
        if (__s2.__tag === "Err") {
          const e = __s2._0;
          return ("error: " + e);
        }
      })();
    }
    if (__s1.__tag === "None") {
      return (() => {
        const __s3 = list_notes(db);
        if (__s3.__tag === "Ok") {
          const notes = __s3._0;
          return render(notes);
        }
        if (__s3.__tag === "Err") {
          const e = __s3._0;
          return ("error: " + e);
        }
      })();
    }
  })();
}
export function main() {
  let db = { __struct: "Db", __proto__: Db$ };
  migrate(db);
  add_note(db, "hello");
  add_note(db, "world");
  http.serve(38573, (path) => handle(db, path));
}
