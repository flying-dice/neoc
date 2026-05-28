const Ok = (v) => ({ __enum: "Result", __tag: "Ok", _0: v });
const Err = (e) => ({ __enum: "Result", __tag: "Err", _0: e });
const Some = (v) => ({ __enum: "Option", __tag: "Some", _0: v });
const None = { __enum: "Option", __tag: "None" };

export function list_notes(db) {
  let params = [];
  const __try_0 = db.query("SELECT id, body FROM notes ORDER BY id", params);
  if (__try_0.__tag === "Err" || __try_0.__tag === "None") return __try_0;
  let rows = __try_0._0;
  let out = [];
  let next = 0;
  for (const row of rows) {
    const __try_1 = decode_list_notes(row);
    if (__try_1.__tag === "Err" || __try_1.__tag === "None") return __try_1;
    let decoded = __try_1._0;
    out[next] = decoded;
    next = (next + 1);
  }
  return Ok(out);
}
function decode_list_notes(cols) {
  const __try_2 = decode_number(cols[0], "list_notes: column 0 (id)");
  if (__try_2.__tag === "Err" || __try_2.__tag === "None") return __try_2;
  let id = __try_2._0;
  const __try_3 = decode_string(cols[1], "list_notes: column 1 (body)");
  if (__try_3.__tag === "Err" || __try_3.__tag === "None") return __try_3;
  let body = __try_3._0;
  return Ok({ __struct: "Note", id: id, body: body });
}
export function add_note(db, body) {
  let params = [{ __enum: "SqlValue", __tag: "Text", _0: body }];
  return db.execute("INSERT INTO notes (body) VALUES (?)", params);
}
function decode_number(value, label) {
  return (() => {
    const __s1 = value;
    if (((__s1.__tag === "Int") || (__s1.__tag === "Real"))) {
      const inner = __s1._0;
      return Ok(inner);
    }
    if (true) {
      return Err(label);
    }
  })();
}
function decode_string(value, label) {
  return (() => {
    const __s2 = value;
    if (__s2.__tag === "Text") {
      const inner = __s2._0;
      return Ok(inner);
    }
    if (true) {
      return Err(label);
    }
  })();
}
