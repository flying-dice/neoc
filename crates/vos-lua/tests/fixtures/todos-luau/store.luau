--!strict
-- What the Todos backend did to its database while it was written in Luau (apps/todos/server/store.luau as of
-- ea14c86a^, the commit before the port to TypeScript): its migration, byte for byte, and the statement its
-- `store.add` kept a todo with. The upgrade test (tests/todos.rs) runs this against a database file, then loads
-- today's backend on the same file. Never edit the migration: its bytes are what existing databases have recorded.
local db = require("vos:app").db

db:migrate {
  [1] = [[
    create table lists (
      id integer primary key,
      name text not null,
      color text not null
    );
    create table todos (
      id integer primary key,
      list_id integer not null,
      title text not null,
      notes text not null default '',
      due text not null default '',
      flagged integer not null default 0,
      done integer not null default 0,
      completed_at text
    );
    create index todos_by_list on todos(list_id, id);
    create index todos_by_completion on todos(done, completed_at desc, id desc);
    insert into lists(name, color) values ('Reminders', 'blue');
  ]],
}

db:exec(
  "insert into todos(list_id, title, notes, due, flagged) values (:l, :t, :n, :d, :f)",
  { l = 1, t = "Kept since the Luau backend", n = "", d = "2026-06-09", f = 1 }
)
