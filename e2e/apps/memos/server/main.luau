--!strict
-- The backend of the fixture app the Discover journeys install, open and remove (`client/e2e/discover.spec.ts`): the
-- test registry offers it and nothing else installs it, so they never disturb the apps the other specs share. It is a
-- short list of memos in the app's own database, and no more: the journeys are about the app, not what it does.
local trim = require("lib:text").trim
local db = app.db

db:migrate {
  [1] = [[
    create table notes (
      id integer primary key,
      body text not null
    );
  ]],
}

local function list(): { memos_Note }
  local notes: { memos_Note } = {}
  for _, row in db:all("select id, body from notes order by id desc") do
    table.insert(notes, { id = row.id, body = row.body })
  end
  return notes
end

RPC.on_request("_list", function(): memos_ListResult
  return list()
end)

RPC.on_request("_add", function(params: memos_AddParams, ctx: vos_app_Context): memos_AddResult
  local body = trim(params.body)
  if body == "" then
    return ctx:fail("A note needs some text.")
  end
  db:exec("insert into notes (body) values (:body)", { body = body })
  return list()
end)
