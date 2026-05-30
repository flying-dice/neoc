export function add_note(db, body) {
  let params = [{ __enum: "SqlValue", __tag: "Text", _0: body }];
  return db.execute("INSERT INTO notes (body) VALUES (?)", params);
}
