import type { Result, Option } from "./neoc-lib";

export interface HttpResponse { status: number; body: string; }
interface NoteLine { id: number; body: string; }

const __t = {};
import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

import { migrate } from "./autosql/migrate";
import { add_note } from "./autosql/add_note";
import { list_notes } from "./autosql/list_notes";
const NoteLine$ = {};
NoteLine$.display = function() {
  return String(this.id) + ", " + String(this.body);
};
__t.NoteLine = ["Display"];
export function init<E>(db: E): Result<number, string> {
  return migrate(db);
}
function render(notes: ListNotesRow[]): string {
  let out = "";
  for (const n of notes) {
    let line = { __struct: "NoteLine", id: n.id, body: n.body, __proto__: NoteLine$ };
    out = ((out + line.display()) + " | ");
  }
  return out;
}
export function list<E>(db: E): HttpResponse {
  return (() => {
    const __s1 = list_notes(db);
    if (__s1.__tag === "Ok") {
      const notes = __s1._0;
      return { __struct: "HttpResponse", status: 200, body: render(notes) };
    }
    if (__s1.__tag === "Err") {
      const e = __s1._0;
      return { __struct: "HttpResponse", status: 500, body: ("error: " + e) };
    }
  })();
}
export function create<E>(db: E, body: string): HttpResponse {
  return (() => {
    if ((body === "")) {
      return { __struct: "HttpResponse", status: 400, body: "body must not be empty" };
    } else {
      return (() => {
        const __s2 = add_note(db, body);
        if (__s2.__tag === "Ok") {
          return { __struct: "HttpResponse", status: 200, body: "added" };
        }
        if (__s2.__tag === "Err") {
          const e = __s2._0;
          return { __struct: "HttpResponse", status: 500, body: ("error: " + e) };
        }
      })();
    }
  })();
}
