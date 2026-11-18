--!strict
-- Sample app for the host end-to-end tests: a small Notes data model with live sources, a two-way bound
-- row and agent tools (no window: the view journeys use the Notes and Calculator of the spec).
-- Type-checked by `bun run lint:luau`.
local app = require("@agentos/app")
local t = app.t
local db = app.db

app.manifest {
  id = "notes",
  name = "Notes",
  icon = "note.text",
  tint = "yellow",
  window = { w = 960, h = 640, minW = 480, minH = 360 },
  permissions = { "data", "notify" },
}

db:migrate {
  [1] = [[
    create table notes(
      id integer primary key,
      body text not null,
      rev integer not null default 0,
      updated_at text,
      author text
    );
  ]],
}

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
app.tools.register {
  name = "add_note",
  title = "Add Note",
  description = "Add a note",
  input = { body = t.string { description = "The note text" } },
  run = function(input, ctx)
    local r = db:exec(
      "insert into notes(body, updated_at, author) values (:body, :now, :who)",
      { body = input.body, now = ctx.now, who = ctx.actor.name }
    )
    app.notify { title = "Note added", body = input.body }
    return { id = r.last_insert_id }
  end,
}

app.tools.register {
  name = "append",
  title = "Append to Note",
  description = "Append text on a new line at the end of a note",
  input = { id = t.integer { description = "Note id" }, text = t.string {} },
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
}

app.tools.register {
  name = "search",
  title = "Search Notes",
  readOnly = true,
  description = "Find notes whose text contains the query",
  input = { query = t.string {}, limit = t.integer { default = 10, min = 1, max = 50 } },
  run = function(input)
    return db:all(
      "select id, substr(body, 1, 60) as title from notes where body like '%' || :q || '%' order by id limit :n",
      { q = input.query, n = input.limit }
    )
  end,
}
