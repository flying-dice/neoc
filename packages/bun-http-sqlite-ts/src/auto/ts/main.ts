import type { Result, Option } from "./neoc-lib";

export interface HttpResponse { status: number; body: string; }
interface NoteLine { id: number; body: string; display(): string; }

const __t: Record<string, string[]> = {};
import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib";

import type { SqlExecutor } from "./autosql/ports";
import { migrate } from "./autosql/migrate";
import { add_note } from "./autosql/add_note";
import type { ListNotesRow } from "./autosql/list_notes";
import { list_notes } from "./autosql/list_notes";
const NoteLine$: Record<string, Function> = {};
NoteLine$.display = function(this: NoteLine) {
  return String(this.id) + ", " + String(this.body);
};
__t.NoteLine = ["Display"];
export function init<E extends SqlExecutor>(db: E): Result<number, string> {
  return migrate(db);
}
function render(notes: ListNotesRow[]): string {
  let out = "";
  for (const n of notes) {
    let line = { __struct: "NoteLine", id: n.id, body: n.body, __proto__: NoteLine$ } as unknown as NoteLine;
    out = ((out + line.display()) + " | ");
  }
  return out;
}
export function list<E extends SqlExecutor>(db: E): HttpResponse {
  return (() => {
    const __s1 = list_notes(db);
    if (__s1.__tag === "Ok") {
      const notes = __s1._0;
      return { __struct: "HttpResponse", status: 200, body: render(notes) };
    }
    const e = __s1._0;
    return { __struct: "HttpResponse", status: 500, body: ("error: " + e) };
  })();
}
export function create<E extends SqlExecutor>(db: E, body: string): HttpResponse {
  return (() => {
    if ((body === "")) {
      return { __struct: "HttpResponse", status: 400, body: "body must not be empty" };
    } else {
      return (() => {
        const __s2 = add_note(db, body);
        if (__s2.__tag === "Ok") {
          return { __struct: "HttpResponse", status: 200, body: "added" };
        }
        const e = __s2._0;
        return { __struct: "HttpResponse", status: 500, body: ("error: " + e) };
      })();
    }
  })();
}
