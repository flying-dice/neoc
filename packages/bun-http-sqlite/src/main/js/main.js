import * as sqlite from "bun:sqlite";
const __t = {};
const Ok = (v) => ({ __enum: "Result", __tag: "Ok", _0: v });
const Err = (e) => ({ __enum: "Result", __tag: "Err", _0: e });
const Some = (v) => ({ __enum: "Option", __tag: "Some", _0: v });
const None = { __enum: "Option", __tag: "None" };

import { migrate } from "./autosql/migrate.js";
import { list_notes } from "./autosql/list_notes.js";
import { add_note } from "./autosql/add_note.js";
const BunSqlite$ = {};
__t.BunSqlite = ["SqlExecutor"];
function num_str(n) {
  let u = n;
  return u.toString();
}
function sql_literal(v) {
  return (() => {
    const __s1 = v;
    if (__s1.__tag === "Int") {
      const n = __s1._0;
      return num_str(n);
    }
    if (__s1.__tag === "Real") {
      const n = __s1._0;
      return num_str(n);
    }
    if (__s1.__tag === "Text") {
      const s = __s1._0;
      return (("'" + s) + "'");
    }
    if (__s1.__tag === "Bool") {
      const b = __s1._0;
      return (() => {
        if (b) {
          return "1";
        } else {
          return "0";
        }
      })();
    }
    if (__s1.__tag === "Null") {
      return "NULL";
    }
  })();
}
function bind(sql, params) {
  let s = sql;
  for (const p of params) {
    let idx = s.indexOf("?");
    let head = s.slice(0, idx);
    let tail = s.slice((idx + 1));
    s = ((head + sql_literal(p)) + tail);
  }
  return s;
}
function cell(value) {
  let kind = value.constructor.name;
  return (() => {
    if ((kind === "Number")) {
      return { __enum: "SqlValue", __tag: "Real", _0: value };
    } else if ((kind === "String")) {
      return { __enum: "SqlValue", __tag: "Text", _0: value };
    } else if ((kind === "Boolean")) {
      return { __enum: "SqlValue", __tag: "Bool", _0: value };
    } else {
      return { __enum: "SqlValue", __tag: "Null" };
    }
  })();
}
BunSqlite$.execute = function(sql, params) {
  let changes = this.db.run(bind(sql, params));
  return Ok(changes.changes);
}
BunSqlite$.query = function(sql, params) {
  let raw = this.db.query(bind(sql, params)).values();
  let out = [];
  let ri = 0;
  for (const row of raw) {
    let cols = [];
    let ci = 0;
    for (const c of row) {
      cols[ci] = cell(c);
      ci = (ci + 1);
    }
    out[ri] = cols;
    ri = (ri + 1);
  }
  return Ok(out);
}
function render(notes) {
  let out = "";
  for (const n of notes) {
    out = ((((out + num_str(n.id)) + ":") + n.body) + " | ");
  }
  return out;
}
function handle(db, req) {
  let url = req.url;
  return (() => {
    if (url.includes("/add/")) {
      let body = url.split("/add/")[1];
      return (() => {
        const __s2 = add_note(db, body);
        if (__s2.__tag === "Ok") {
          return new Response(("added: " + body));
        }
        if (__s2.__tag === "Err") {
          const e = __s2._0;
          return new Response(("error: " + e));
        }
      })();
    } else {
      return (() => {
        const __s3 = list_notes(db);
        if (__s3.__tag === "Ok") {
          const notes = __s3._0;
          return new Response(render(notes));
        }
        if (__s3.__tag === "Err") {
          const e = __s3._0;
          return new Response(("error: " + e));
        }
      })();
    }
  })();
}
export function main() {
  let raw = new sqlite.Database(":memory:");
  let db = { __struct: "BunSqlite", db: raw, __proto__: BunSqlite$ };
  migrate(db);
  add_note(db, "hello");
  add_note(db, "world");
  Bun.serve({ port: 38573, fetch: (req) => handle(db, req) });
}
