--!strict
-- Sample app: notes. Type-checked by `bun run lint:luau` against types/agentos.d.luau.
local db = app.data

app.tools.register({
  name = "add_note",
  description = "Add a note",
  input_schema = { type = "object", properties = { body = { type = "string" } }, required = { "body" } },
  handler = function(args: { body: string }): { id: number }
    local r = db.exec("INSERT INTO notes(body) VALUES(?)", { args.body })
    app.notify({ title = "Note added", body = args.body })
    return { id = r.last_insert_rowid }
  end,
})

app.on("ui:event", function(ev: { type: string, id: number? }): any
  if ev.type == "list" then
    return db.query("SELECT id, body FROM notes ORDER BY id")
  end
  return nil
end)
