--!strict
-- The backend of the fixture app's next version, 0.2.0 (e2e/apps/journal is 0.1.0): a second migration says which version
-- wrote each entry, so the update journey (`e2e/updates.journeys.spec.ts`) sees the entries of 0.1.0 kept and migrated.
local db = app.db

db:migrate {
  [1] = [[
    create table entries (
      id integer primary key,
      body text not null
    );
  ]],
  [2] = [[
    alter table entries add column edition text not null default '0.1.0';
  ]],
}

local function list(): { journal_Entry }
  local entries: { journal_Entry } = {}
  for _, row in db:all("select id, body, edition from entries order by id") do
    table.insert(entries, { id = row.id, body = row.body, edition = row.edition })
  end
  return entries
end

RPC.on_request("_list", function(): journal_ListResult
  return list()
end)

RPC.on_request("_add", function(params: journal_AddParams): journal_AddResult
  db:exec("insert into entries (body, edition) values (:body, '0.2.0')", { body = params.body })
  return list()
end)
