--!strict
-- Sample backend for the host end-to-end tests: a small Notes data model, the handlers its page calls (private, `_`)
-- and the tools an agent calls (public, described by `notes.openrpc.json`). Whoever changes a note tells the open pages with a notification, so a page
-- reads again.
local app = require("vos:app")
local RPC = require("vos:rpc")
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
  RPC.send_notification("notes.changed", { id = id })
end

-- tools
RPC.on_request("add_note", function(input: { body: string }, ctx)
  local r = db:exec(
    "insert into notes(body, updated_at, author) values (:body, :now, :who)",
    { body = input.body, now = ctx.now, who = ctx.actor.name }
  )
  app.notify { title = "Note added", body = input.body }
  changed(r.last_insert_id)
  return { id = r.last_insert_id }
end)

RPC.on_request("append", function(input: { id: number, text: string }, ctx)
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
end)

RPC.on_request("search", function(input: { query: string, limit: number? })
  return db:all(
    "select id, substr(body, 1, 60) as title from notes where body like '%' || :q || '%' order by id limit :n",
    { q = input.query, n = input.limit or 10 }
  )
end)

-- what the page calls
RPC.on_request("_list", function()
  return db:all("select id, substr(body, 1, 60) as title, updated_at from notes order by id")
end)

RPC.on_request("_get", function(args: { id: number })
  return db:one("select id, body, rev, updated_at, author from notes where id = :id", { id = args.id })
end)

RPC.on_request("_save", function(args: { id: number, body: string }, ctx)
  local r = db:exec(
    "update notes set body = :v, rev = rev + 1, updated_at = :now, author = :who where id = :id",
    { v = args.body, id = args.id, now = ctx.now, who = ctx.actor.name }
  )
  if r.changes == 0 then
    return ctx:fail("No note with id " .. args.id)
  end
  changed(args.id)
  return db:one("select body, rev, author from notes where id = :id", { id = args.id })
end)
