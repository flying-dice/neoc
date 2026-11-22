--!strict
-- The lists and their todos, in the app's own database. Everything that changes them is here, and says why when it
-- cannot: a function that changes something answers with a plain sentence when it refused (the handler and the tool
-- pass it on as it is), and with nothing when it did what it was asked.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local db = app.db

-- what can change on a todo: only what is given changes
export type Changes = { title: string?, notes: string?, due: string?, flagged: boolean? }

local store = {}

-- completed todos are kept, but a board carries only the most recent of them
local COMPLETED_SHOWN = 500

local TODO = "select id, list_id, title, notes, due, flagged, done from todos"

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

local function trim(text: string): string
  return string.match(text, "^%s*(.-)%s*$") :: string
end

-- a day is `2026-06-09`; empty is no day
local function isDay(text: string): boolean
  if text == "" then
    return true
  end
  local month, day = string.match(text, "^%d%d%d%d%-(%d%d)%-(%d%d)$")
  if month == nil or day == nil then
    return false
  end
  local m, d = tonumber(month) :: number, tonumber(day) :: number
  return m >= 1 and m <= 12 and d >= 1 and d <= 31
end

local function todoOf(row: { [string]: any }): rpc.Todo
  return {
    id = row.id,
    list = row.list_id,
    title = row.title,
    notes = row.notes,
    due = row.due,
    flagged = row.flagged == 1,
    done = row.done == 1,
  }
end

function store.lists(): { rpc.List }
  local lists: { rpc.List } = {}
  for _, row in db:all("select id, name, color from lists order by id") do
    table.insert(lists, { id = row.id, name = row.name, color = row.color })
  end
  return lists
end

-- the list called `name`, whatever its capitals: how an agent names a list
function store.listNamed(name: string): rpc.List?
  local wanted = string.lower(trim(name))
  for _, list in store.lists() do
    if string.lower(list.name) == wanted then
      return list
    end
  end
  return nil
end

function store.todo(id: number): rpc.Todo?
  local row = db:one(TODO .. " where id = :id", { id = id })
  return if row then todoOf(row) else nil
end

-- the open todos in the order they were added
function store.open(): { rpc.Todo }
  local todos: { rpc.Todo } = {}
  for _, row in db:all(TODO .. " where done = 0 order by id") do
    table.insert(todos, todoOf(row))
  end
  return todos
end

-- the completed todos, the most recently completed first
function store.completed(): { rpc.Todo }
  local todos: { rpc.Todo } = {}
  for _, row in db:all(TODO .. " where done = 1 order by completed_at desc, id desc limit :n", { n = COMPLETED_SHOWN }) do
    table.insert(todos, todoOf(row))
  end
  return todos
end

function store.board(): rpc.Board
  local todos = store.open()
  table.move(store.completed(), 1, COMPLETED_SHOWN, #todos + 1, todos)
  return { lists = store.lists(), todos = todos }
end

function store.addList(name: string, color: rpc.Color): string?
  local named = trim(name)
  if named == "" then
    return "A list needs a name."
  end
  db:exec("insert into lists(name, color) values (:n, :c)", { n = named, c = color })
  return nil
end

function store.removeList(id: number): string?
  if not db:one("select id from lists where id = :id", { id = id }) then
    return "There is no such list."
  end
  local count = db:one("select count(*) as n from lists") :: { [string]: any }
  if count.n <= 1 then
    return "The last list cannot be deleted."
  end
  db:tx(function()
    db:exec("delete from todos where list_id = :id", { id = id })
    db:exec("delete from lists where id = :id", { id = id })
    return nil
  end)
  return nil
end

-- Add a todo: answers with its id, or with nothing and why not.
function store.add(list: number, title: string, with: Changes?): (number?, string?)
  local given: Changes = with or {}
  local titled = trim(title)
  local due = given.due or ""
  if titled == "" then
    return nil, "A todo needs a title."
  elseif not isDay(due) then
    return nil, "A day is written like 2026-06-09."
  elseif not db:one("select id from lists where id = :id", { id = list }) then
    return nil, "There is no such list."
  end
  local made = db:exec(
    "insert into todos(list_id, title, notes, due, flagged) values (:l, :t, :n, :d, :f)",
    { l = list, t = titled, n = given.notes or "", d = due, f = if given.flagged then 1 else 0 }
  )
  return made.last_insert_id, nil
end

function store.update(id: number, changes: Changes): string?
  local todo = store.todo(id)
  if not todo then
    return "There is no such todo."
  end
  local title = if changes.title ~= nil then trim(changes.title) else todo.title
  local due = if changes.due ~= nil then changes.due else todo.due
  if title == "" then
    return "A todo needs a title."
  elseif not isDay(due) then
    return "A day is written like 2026-06-09."
  end
  local flagged = todo.flagged
  if changes.flagged ~= nil then
    flagged = changes.flagged
  end
  db:exec("update todos set title = :t, notes = :n, due = :d, flagged = :f where id = :id", {
    id = id,
    t = title,
    n = if changes.notes ~= nil then changes.notes else todo.notes,
    d = due,
    f = if flagged then 1 else 0,
  })
  return nil
end

-- Complete a todo (`done`), or bring it back.
function store.complete(id: number, done: boolean): string?
  local changed = db:exec(
    [[update todos
      set done = :d,
          completed_at = case when :d = 1 then strftime('%Y-%m-%dT%H:%M:%fZ', 'now') else null end
      where id = :id]],
    { id = id, d = if done then 1 else 0 }
  )
  return if changed.changes == 0 then "There is no such todo." else nil
end

function store.remove(id: number): string?
  local changed = db:exec("delete from todos where id = :id", { id = id })
  return if changed.changes == 0 then "There is no such todo." else nil
end

return store
