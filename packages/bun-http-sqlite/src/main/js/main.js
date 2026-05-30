import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";

import { migrate } from "./autosql/migrate.js";
import { add_note } from "./autosql/add_note.js";
import { list_notes } from "./autosql/list_notes.js";
export function init(db) {
  return migrate(db);
}
export function list(db) {
  return list_notes(db);
}
export function create(db, body) {
  return (() => {
    if ((body === "")) {
      return Err("body must not be empty");
    } else {
      return add_note(db, body);
    }
  })();
}
