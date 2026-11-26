--!strict
-- The notebooks and their notes, in the app's own database. Everything that changes them is here, and says why when
-- it cannot: a function that changes something answers with a plain sentence when it refused (the handler and the
-- tool pass it on as it is), and with nothing when it did what it was asked.
--
-- A note's title is its name in its notebook: no two notes of a notebook have titles that differ only in capitals,
-- so a link (`[[Title]]`) names one note at most. What a note links to is kept beside it (`links`), as the keys of
-- the titles its text names, whether or not there is such a note yet: a link to a note that is made later leads to it
-- from then on.
local strings = require("lib:text")
local text = require("./text")
local unicode = require("lib:unicode")
local db = app.db

local store = {}

-- a search answers with the best of what it found
local HITS = 50

local NOW = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
local SUMMARY = "select id, notebook_id, title, excerpt, tags, pinned, updated_at from notes"

db:migrate {
  [1] = [[
    create table notebooks (
      id integer primary key,
      name text not null
    );
    create table notes (
      id integer primary key,
      notebook_id integer not null,
      title text not null,
      title_key text not null,
      body text not null default '',
      excerpt text not null default '',
      tags text not null default '',
      pinned integer not null default 0,
      updated_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    create unique index notes_by_title on notes(notebook_id, title_key);
    create index notes_by_change on notes(updated_at desc, id desc);
    create table links (
      note_id integer not null,
      target text not null,
      primary key (note_id, target)
    ) without rowid;
    create index links_by_target on links(target);
    insert into notebooks(name) values ('Notebook');
  ]],
}

local function summaryOf(row: { [string]: any }): notebooks_Summary
  return {
    id = row.id,
    notebook = row.notebook_id,
    title = row.title,
    excerpt = row.excerpt,
    -- the tags are kept as one text, a space between them: a tag has no space in it
    tags = if row.tags == "" then {} else string.split(row.tags, " "),
    pinned = row.pinned == 1,
    updated = row.updated_at,
  }
end

function store.notebooks(): { notebooks_Notebook }
  local notebooks: { notebooks_Notebook } = {}
  for _, row in db:all("select id, name from notebooks order by id") do
    table.insert(notebooks, { id = row.id, name = row.name })
  end
  return notebooks
end

-- the notebook called `name`, whatever its capitals: how an agent names a notebook
function store.notebookNamed(name: string): notebooks_Notebook?
  local wanted = strings.trim(name)
  for _, notebook in store.notebooks() do
    if unicode.caseless_eq(notebook.name, wanted) then
      return notebook
    end
  end
  return nil
end

local function hasNotebook(id: number): boolean
  return db:one("select id from notebooks where id = :id", { id = id }) ~= nil
end

-- The notes without their text, the most recently changed first; of one notebook when it is given.
function store.summaries(notebook: number?): { notebooks_Summary }
  local notes: { notebooks_Summary } = {}
  local rows = if notebook ~= nil
    then db:all(SUMMARY .. " where notebook_id = :nb order by updated_at desc, id desc", { nb = notebook })
    else db:all(SUMMARY .. " order by updated_at desc, id desc")
  for _, row in rows do
    table.insert(notes, summaryOf(row))
  end
  return notes
end

function store.library(): notebooks_Library
  local links: { notebooks_Link } = {}
  -- a link leads to the note of the same notebook whose title it names; a note's link to itself is not drawn
  local rows = db:all([[
    select l.note_id as src, t.id as dst
    from links l
    join notes n on n.id = l.note_id
    join notes t on t.notebook_id = n.notebook_id and t.title_key = l.target
    where t.id <> n.id
    order by l.note_id, t.id
  ]])
  for _, row in rows do
    table.insert(links, { from = row.src, to = row.dst })
  end
  return { notebooks = store.notebooks(), notes = store.summaries(nil), links = links }
end

function store.note(id: number): notebooks_Note?
  local row =
    db:one("select id, notebook_id, title, title_key, body, pinned, updated_at from notes where id = :id", { id = id })
  if not row then
    return nil
  end
  local backlinks: { notebooks_Backlink } = {}
  local linking = db:all(
    [[select n.id, n.title, n.body from notes n join links l on l.note_id = n.id
      where l.target = :key and n.notebook_id = :nb and n.id <> :id
      order by n.title_key]],
    { key = row.title_key, nb = row.notebook_id, id = id }
  )
  for _, from in linking do
    table.insert(backlinks, { id = from.id, title = from.title, excerpt = text.mention(from.body, row.title_key) })
  end
  return {
    id = row.id,
    notebook = row.notebook_id,
    title = row.title,
    body = row.body,
    pinned = row.pinned == 1,
    updated = row.updated_at,
    backlinks = backlinks,
  }
end

-- The id of the note of a notebook with this title, whatever its capitals.
function store.noteNamed(notebook: number, title: string): number?
  local row = db:one(
    "select id from notes where notebook_id = :nb and title_key = :key",
    { nb = notebook, key = text.key(title) }
  )
  return if row then row.id else nil
end

-- Keep what a note's text says about it: its first line, its tags and what it links to. Inside a transaction.
local function index(id: number, body: string)
  db:exec("update notes set body = :b, excerpt = :e, tags = :t, updated_at = " .. NOW .. " where id = :id", {
    id = id,
    b = body,
    e = text.excerpt(body),
    t = table.concat(text.tags(body), " "),
  })
  db:exec("delete from links where note_id = :id", { id = id })
  for _, target in text.links(body) do
    db:exec("insert into links(note_id, target) values (:id, :t)", { id = id, t = target })
  end
end

-- the first of Untitled, Untitled 2, Untitled 3... that no note of the notebook has
local function untitled(notebook: number): string
  local title = "Untitled"
  local count = 1
  while store.noteNamed(notebook, title) ~= nil do
    count += 1
    title = `Untitled {count}`
  end
  return title
end

-- Make a note: answers with its id, or with nothing and why not. Without a title it is called Untitled.
function store.create(notebook: number, title: string?, body: string?): (number?, string?)
  if not hasNotebook(notebook) then
    return nil, "There is no such notebook."
  end
  local named = if title ~= nil then strings.trim(title) else untitled(notebook)
  local problem = text.titleProblem(named)
  if problem then
    return nil, problem
  elseif store.noteNamed(notebook, named) ~= nil then
    return nil, `There is already a note called {named}.`
  end
  local id: number? = nil
  db:tx(function()
    local made = db:exec(
      "insert into notes(notebook_id, title, title_key) values (:nb, :t, :k)",
      { nb = notebook, t = named, k = text.key(named) }
    )
    id = made.last_insert_id
    if body ~= nil and body ~= "" then
      index(made.last_insert_id, body)
    end
    return nil
  end)
  return id, nil
end

-- The note a link names: the one with this title, or a new one. Answers with its id, or with nothing and why not.
function store.follow(notebook: number, title: string): (number?, string?)
  local found = store.noteNamed(notebook, title)
  if found ~= nil then
    return found, nil
  end
  return store.create(notebook, title, nil)
end

function store.save(id: number, body: string): string?
  if not db:one("select id from notes where id = :id", { id = id }) then
    return "There is no such note."
  end
  db:tx(function()
    index(id, body)
    return nil
  end)
  return nil
end

-- Give a note another title, and rewrite the links to it in the other notes of its notebook.
function store.rename(id: number, title: string): string?
  local row = db:one("select notebook_id, title, title_key from notes where id = :id", { id = id })
  if not row then
    return "There is no such note."
  end
  local named = strings.trim(title)
  local key = text.key(named)
  local problem = text.titleProblem(named)
  if problem then
    return problem
  end
  local other = store.noteNamed(row.notebook_id, named)
  if other ~= nil and other ~= id then
    return `There is already a note called {named}.`
  elseif named == row.title then
    return nil
  end
  db:tx(function()
    db:exec("update notes set title = :t, title_key = :k, updated_at = " .. NOW .. " where id = :id", {
      id = id,
      t = named,
      k = key,
    })
    -- the note's own links to itself are rewritten with the others
    local linking = db:all(
      [[select n.id, n.body from notes n join links l on l.note_id = n.id
        where l.target = :key and n.notebook_id = :nb]],
      { key = row.title_key, nb = row.notebook_id }
    )
    for _, from in linking do
      index(from.id, text.retarget(from.body, row.title_key, named))
    end
    return nil
  end)
  return nil
end

function store.pin(id: number, pinned: boolean): string?
  local changed = db:exec("update notes set pinned = :p where id = :id", { id = id, p = if pinned then 1 else 0 })
  return if changed.changes == 0 then "There is no such note." else nil
end

function store.remove(id: number): string?
  local removed = 0
  db:tx(function()
    db:exec("delete from links where note_id = :id", { id = id })
    removed = db:exec("delete from notes where id = :id", { id = id }).changes
    return nil
  end)
  return if removed == 0 then "There is no such note." else nil
end

function store.addNotebook(name: string): string?
  local named = strings.trim(name)
  if named == "" then
    return "A notebook needs a name."
  elseif store.notebookNamed(named) ~= nil then
    return `There is already a notebook called {named}.`
  end
  db:exec("insert into notebooks(name) values (:n)", { n = named })
  return nil
end

function store.removeNotebook(id: number): string?
  if not hasNotebook(id) then
    return "There is no such notebook."
  end
  local count = db:one("select count(*) as n from notebooks")
  if count == nil or count.n <= 1 then
    return "The last notebook cannot be deleted."
  end
  db:tx(function()
    db:exec("delete from links where note_id in (select id from notes where notebook_id = :id)", { id = id })
    db:exec("delete from notes where notebook_id = :id", { id = id })
    db:exec("delete from notebooks where id = :id", { id = id })
    return nil
  end)
  return nil
end

-- The notes of a notebook with `query` in their title or their text, the ones with it in their title first. Each
-- comes with the line the words are in.
function store.search(notebook: number, query: string): { notebooks_Hit }
  local words = strings.trim(query)
  local hits: { notebooks_Hit } = {}
  if words == "" then
    return hits
  end
  -- `like` reads % and _ as wildcards: the words are looked for as they are written
  local pattern = "%" .. string.gsub(words, "[\\%%_]", "\\%0") .. "%"
  local rows = db:all(
    [[select id, title, body, excerpt, title like :q escape '\' as titled from notes
      where notebook_id = :nb and (title like :q escape '\' or body like :q escape '\')
      order by titled desc, updated_at desc, id desc limit :n]],
    { nb = notebook, q = pattern, n = HITS }
  )
  for _, row in rows do
    local line = text.line(row.body, words)
    table.insert(hits, { id = row.id, title = row.title, excerpt = if line ~= "" then line else row.excerpt })
  end
  return hits
end

return store
