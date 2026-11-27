--!strict
-- The backend of the fixture app the update journey installs at 0.1.0, writes in and updates (`e2e/updates.journeys.spec.ts`).
-- Its next version is e2e/apps/journal-next. Nothing installs it at start and no registry lists it.
local db = app.db

db:migrate {
  [1] = [[
    create table entries (
      id integer primary key,
      body text not null
    );
  ]],
}

local function list(): { journal_Entry }
  local entries: { journal_Entry } = {}
  for _, row in db:all("select id, body from entries order by id") do
    table.insert(entries, { id = row.id, body = row.body })
  end
  return entries
end

RPC.on_request("_list", function(): journal_ListResult
  return list()
end)

RPC.on_request("_add", function(params: journal_AddParams): journal_AddResult
  db:exec("insert into entries (body) values (:body)", { body = params.body })
  return list()
end)
