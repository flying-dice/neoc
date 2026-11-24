--!strict
-- Sample backend for the host end-to-end tests: a small Notes data model, the handlers its page calls and the tools
-- an agent calls. Whoever changes a note tells the open pages with an event, so a page reads again.
local app = require("vos:app")
local t = app.t
local db = app.db

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

local function changed(id: number)
  app.emit("notes.changed", { id = id })
end

-- tools
app.tool {
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
    changed(r.last_insert_id)
    return { id = r.last_insert_id }
  end,
}

app.tool {
  name = "append",
  title = "Append to Note",
  description = "Append text on a new line at the end of a note",
  input = { id = t.integer { description = "Note id" }, text = t.string {} },
  run = function(input: { id: number, text: string }, ctx)
    local r = db:exec(
      [[update notes set body = body || char(10) || :text, rev = rev + 1, updated_at = :now, author = :who
        where id = :id]],
      { text = input.text, id = input.id, now = ctx.now, who = ctx.actor.name }
    )
    if r.changes == 0 then
      return ctx:fail(`No note with id {input.id}`)
    end
    changed(input.id)
    return { id = input.id }
  end,
}

app.tool {
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

-- what the page calls
local handlers: vos_app_Handlers = {
  list = function()
    return db:all("select id, substr(body, 1, 60) as title, updated_at from notes order by id")
  end,
  get = function(_ctx, args)
    return db:one("select id, body, rev, updated_at, author from notes where id = :id", { id = args.id })
  end,
  save = function(ctx, args)
    local r = db:exec(
      "update notes set body = :v, rev = rev + 1, updated_at = :now, author = :who where id = :id",
      { v = args.body, id = args.id, now = ctx.now, who = ctx.actor.name }
    )
    if r.changes == 0 then
      return ctx:fail("No note with id " .. args.id)
    end
    changed(args.id)
    return db:one("select body, rev, author from notes where id = :id", { id = args.id })
  end,
}

return handlers
