--!strict
app.tools.register({
  name = "add_note",
  description = "Add a note",
  input_schema = { type = "object", properties = { body = { type = "string" } }, required = { "body" } },
  handler = function(args: { body: string }): { id: number }
    local r = app.data.exec("INSERT INTO notes(body) VALUES(?)", { args.body })
    app.notify({ title = "Note added", body = args.body })
    return { id = r.last_insert_rowid }
  end,
})

local function view(): any
  local rows = app.data.query("SELECT id, body FROM notes ORDER BY id")
  local items = {}
  for _, r in rows do
    table.insert(items, { key = tostring(r.id), label = r.body })
  end
  local children: { any } = {
    { type = "text", text = "Notes: " .. #rows },
    { type = "list", id = "notes", items = items },
  }
  return { type = "stack", direction = "column", children = children }
end

app.on("ui:event", function(ev: any): any
  if ev.name == "add" then
    app.data.exec("INSERT INTO notes(body) VALUES(?)", { ev.data.body })
  end
  return view()
end)
