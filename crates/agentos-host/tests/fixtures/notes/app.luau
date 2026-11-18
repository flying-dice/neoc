--!strict
-- Sample app for the host end-to-end tests: a small Notes data model with live sources, a two-way bound
-- row, agent tools and the legacy `ui:event` hook. Type-checked by `bun run lint:luau`.
local app = require("@agentos/app")
local t = app.t
local db = app.db

app.manifest({
  id = "notes",
  name = "Notes",
  icon = "note.text",
  tint = "yellow",
  window = { w = 960, h = 640, minW = 480, minH = 360 },
  permissions = { "data", "notify" },
})

db:migrate({
  [1] = [[
    create table notes(
      id integer primary key,
      body text not null,
      rev integer not null default 0,
      updated_at text,
      author text
    );
  ]],
})

-- sources: prepared once at load; the SQL never leaves the host
local noteList =
  db:query([[select id, substr(body, 1, 60) as title, updated_at from notes order by id]], { key = "id", page = 50 })
local noteCount = db:scalar("select count(*) from notes")
local note = db:row("select id, body, rev, updated_at, author from notes where id = :id", {
  key = "id",
  rev = "rev",
  write = function(id: number, field: string, value: any, ctx)
    assert(field == "body", "only the body is two-way bound")
    db:exec(
      "update notes set body = :v, rev = rev + 1, updated_at = :now, author = :who where id = :id",
      { v = value, id = id, now = ctx.now, who = ctx.actor.name }
    )
  end,
})
assert(noteList ~= nil and noteCount ~= nil and note ~= nil)

-- tools
app.tools.register({
  name = "add_note",
  title = "Add Note",
  description = "Add a note",
  input = { body = t.string({ description = "The note text" }) },
  run = function(input, ctx)
    local r = db:exec(
      "insert into notes(body, updated_at, author) values (:body, :now, :who)",
      { body = input.body, now = ctx.now, who = ctx.actor.name }
    )
    app.notify({ title = "Note added", body = input.body })
    return { id = r.last_insert_id }
  end,
})

app.tools.register({
  name = "notes.append",
  title = "Append to Note",
  description = "Append text on a new line at the end of a note",
  input = { id = t.integer({ description = "Note id" }), text = t.string({}) },
  run = function(input, ctx)
    local r = db:exec(
      [[update notes set body = body || char(10) || :text, rev = rev + 1, updated_at = :now, author = :who
        where id = :id]],
      { text = input.text, id = input.id, now = ctx.now, who = ctx.actor.name }
    )
    if r.changes == 0 then
      return ctx:fail(`No note with id {input.id}`)
    end
    return { id = input.id }
  end,
})

app.tools.register({
  name = "notes.search",
  title = "Search Notes",
  readOnly = true,
  description = "Find notes whose text contains the query",
  input = { query = t.string({}), limit = t.integer({ default = 10, min = 1, max = 50 }) },
  run = function(input)
    return db:all(
      "select id, substr(body, 1, 60) as title from notes where body like '%' || :q || '%' order by id limit :n",
      { q = input.query, n = input.limit }
    )
  end,
})

-- legacy view hook (replaced by windows and the view protocol): a flat tree of the notes
app.on("ui:event", function(ev: any): any
  if ev.name == "add" then
    db:exec("insert into notes(body) values (:body)", { body = ev.data.body })
  end
  local rows = db:all("select id, body from notes order by id")
  local items = {}
  for _, r in rows do
    table.insert(items, { key = tostring(r.id), label = r.body })
  end
  local children: { any } = {
    { type = "text", text = "Notes: " .. #rows },
    { type = "list", id = "notes", items = items },
  }
  return { type = "stack", direction = "column", children = children }
end)
