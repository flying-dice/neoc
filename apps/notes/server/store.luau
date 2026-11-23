--!strict
-- The folders and their notes, in the app's own database. Everything that changes them is here, and says why when it
-- cannot: a function that changes something answers with a plain sentence when it refused (the handler and the tool
-- pass it on as it is), and with nothing when it did what it was asked.
--
-- A note is its text: its first line is its title. A deleted note is kept for 30 days (Recently Deleted), from where
-- it can be brought back; a note with nothing in it is not worth keeping, and is gone at once.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local strings = require("lib:text")
local unicode = require("lib:unicode")

local trim = strings.trim
local db = app.db

local store = {}

-- a board carries only the most recently edited notes
local NOTES_SHOWN = 2000
-- how much of a note's text its title and preview are read from, and how long each may be
local HEAD = 400
local LINE = 120
-- the longest a note may be (the contract's `maxLength` of `write`)
store.MAX_BODY = 20000

local NOW = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
local NOTE = "select id, folder_id, substr(body, 1, " .. HEAD .. ") as head, pinned, edited_at, deleted_at from notes"

db:migrate {
  [1] = [[
    create table folders (
      id integer primary key,
      name text not null
    );
    create table notes (
      id integer primary key,
      folder_id integer not null,
      body text not null default '',
      pinned integer not null default 0,
      edited_at text not null,
      deleted_at text
    );
    create index notes_by_edit on notes(edited_at desc, id desc);
    create index notes_by_folder on notes(folder_id);
    insert into folders(name) values ('Notes');
  ]],
}

-- A note's title and preview: its first two lines that say something.
function store.lines(text: string): (string, string)
  local title, preview = "", ""
  for line in string.gmatch(text, "[^\r\n]+") do
    local said = trim(line)
    if said ~= "" then
      if title == "" then
        title = strings.clip(said, LINE)
      else
        preview = strings.clip(said, LINE)
        break
      end
    end
  end
  return title, preview
end

local function noteOf(row: { [string]: any }): rpc.Note
  local title, preview = store.lines(row.head)
  return {
    id = row.id,
    folder = row.folder_id,
    title = title,
    preview = preview,
    pinned = row.pinned == 1,
    edited = row.edited_at,
    deleted = row.deleted_at ~= nil,
  }
end

function store.folders(): { rpc.Folder }
  local folders: { rpc.Folder } = {}
  for _, row in db:all("select id, name from folders order by id") do
    table.insert(folders, { id = row.id, name = row.name })
  end
  return folders
end

-- the folder called `name`, whatever its capitals: how an agent names a folder, and how a name is found taken
function store.folderNamed(name: string): rpc.Folder?
  local wanted = trim(name)
  for _, folder in store.folders() do
    if unicode.caseless_eq(folder.name, wanted) then
      return folder
    end
  end
  return nil
end

function store.note(id: number): rpc.Note?
  local row = db:one(NOTE .. " where id = :id", { id = id })
  return if row then noteOf(row) else nil
end

-- the whole text of a note
function store.body(id: number): string?
  local row = db:one("select body from notes where id = :id", { id = id })
  return if row then row.body else nil
end

-- every note, deleted ones too, the most recently edited first
function store.notes(): { rpc.Note }
  local notes: { rpc.Note } = {}
  for _, row in db:all(NOTE .. " order by edited_at desc, id desc limit :n", { n = NOTES_SHOWN }) do
    table.insert(notes, noteOf(row))
  end
  return notes
end

-- the ids of the notes whose text holds `query`, whatever its capitals; deleted notes are not searched
function store.search(query: string): { number }
  local found: { number } = {}
  local asked = trim(query)
  if asked == "" then
    return found
  end
  local rows = db:all(
    "select id from notes where deleted_at is null and instr(lower(body), lower(:q)) > 0 order by edited_at desc, id desc limit :n",
    { q = asked, n = NOTES_SHOWN }
  )
  for _, row in rows do
    table.insert(found, row.id)
  end
  return found
end

-- Forget the notes that were deleted more than 30 days ago.
function store.sweep()
  db:exec(
    "delete from notes where deleted_at is not null and deleted_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-30 days')"
  )
end

function store.board(): rpc.Board
  return { folders = store.folders(), notes = store.notes() }
end

function store.addFolder(name: string): string?
  local named = trim(name)
  if named == "" then
    return "A folder needs a name."
  elseif store.folderNamed(named) then
    return `There is already a folder called {named}.`
  end
  db:exec("insert into folders(name) values (:n)", { n = named })
  return nil
end

-- Delete a folder: its notes go to Recently Deleted, and belong to the first folder from then on, so one brought back
-- has somewhere to be.
function store.removeFolder(id: number): string?
  if not db:one("select id from folders where id = :id", { id = id }) then
    return "There is no such folder."
  end
  local first = db:one("select min(id) as id from folders") :: { [string]: any }
  if first.id == id then
    return "The first folder cannot be deleted."
  end
  db:tx(function()
    db:exec("delete from notes where folder_id = :id and trim(body, ' ' || char(9, 10, 13)) = ''", { id = id })
    db:exec(
      "update notes set folder_id = :first, pinned = 0, deleted_at = coalesce(deleted_at, "
        .. NOW
        .. ") where folder_id = :id",
      { id = id, first = first.id }
    )
    db:exec("delete from folders where id = :id", { id = id })
    return nil
  end)
  return nil
end

-- Start a note in a folder: answers with its id, or with nothing and why not.
function store.create(folder: number, body: string?): (number?, string?)
  if not db:one("select id from folders where id = :id", { id = folder }) then
    return nil, "There is no such folder."
  end
  local made = db:exec(
    "insert into notes(folder_id, body, edited_at) values (:f, :b, " .. NOW .. ")",
    { f = folder, b = body or "" }
  )
  return made.last_insert_id, nil
end

-- why a note cannot be changed, or nothing when it can
local function unchangeable(id: number): string?
  local note = store.note(id)
  if not note then
    return "There is no such note."
  elseif note.deleted then
    return "A deleted note cannot be changed: recover it first."
  end
  return nil
end

-- Replace a note's text. Text it already has changes nothing, and leaves when it was edited as it was.
function store.write(id: number, body: string): string?
  local problem = unchangeable(id)
  if problem then
    return problem
  elseif utf8.len(body) == nil or (utf8.len(body) :: number) > store.MAX_BODY then
    return `A note holds {store.MAX_BODY} characters at most.`
  end
  db:exec("update notes set body = :b, edited_at = " .. NOW .. " where id = :id and body <> :b", { id = id, b = body })
  return nil
end

function store.pin(id: number, pinned: boolean): string?
  local problem = unchangeable(id)
  if problem then
    return problem
  end
  db:exec("update notes set pinned = :p where id = :id", { id = id, p = if pinned then 1 else 0 })
  return nil
end

function store.move(id: number, folder: number): string?
  local problem = unchangeable(id)
  if problem then
    return problem
  elseif not db:one("select id from folders where id = :id", { id = folder }) then
    return "There is no such folder."
  end
  db:exec("update notes set folder_id = :f where id = :id", { id = id, f = folder })
  return nil
end

-- Delete a note: to Recently Deleted, or for good when there is nothing in it.
function store.remove(id: number): string?
  local body = store.body(id)
  if body == nil then
    return "There is no such note."
  elseif trim(body) == "" then
    db:exec("delete from notes where id = :id", { id = id })
  else
    db:exec("update notes set pinned = 0, deleted_at = coalesce(deleted_at, " .. NOW .. ") where id = :id", { id = id })
  end
  return nil
end

function store.restore(id: number): string?
  local changed = db:exec("update notes set deleted_at = null where id = :id", { id = id })
  return if changed.changes == 0 then "There is no such note." else nil
end

-- Delete a deleted note for good.
function store.purge(id: number): string?
  local note = store.note(id)
  if not note then
    return "There is no such note."
  elseif not note.deleted then
    return "Only a deleted note can be deleted for good."
  end
  db:exec("delete from notes where id = :id", { id = id })
  return nil
end

return store
