--!strict
-- The Notebooks app's backend: notebooks of Markdown notes that link to each other, in the app's own database
-- (store.luau). What a note's text says about it (its links, its tags) is read from the text here (text.luau), so a
-- note written by an agent is linked and tagged as one written in the window is.
--
-- A call that changes something answers with the library as it now stands (and with the note, when the call is about
-- one), so the window that asked draws what the backend has.
--
-- An agent can do what a person does most: see the notes (`list_notes`), read one (`read_note`), write one
-- (`write_note`) and look for words (`search_notes`). A tool that changed something tells the open windows with a
-- `notebooks.changed` event, and they read the library again.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local store = require("./store")
local t = app.t

-- the note with this id and the library, for a call that made, found or renamed it
local function opened(ctx: rpc.Context, id: number?, problem: string?): rpc.Opened
  if problem ~= nil then
    return ctx:fail(problem)
  end
  local note = if id ~= nil then store.note(id) else nil
  if not note then
    return ctx:fail("There is no such note.")
  end
  return { library = store.library(), note = note }
end

local handlers: rpc.Handlers = {
  library = function(_ctx)
    return store.library()
  end,

  read = function(ctx, params)
    local note = store.note(params.note)
    return if note then note else ctx:fail("There is no such note.")
  end,

  create = function(ctx, params)
    local id, problem = store.create(params.notebook, params.title, nil)
    return opened(ctx, id, problem)
  end,

  follow = function(ctx, params)
    local id, problem = store.follow(params.notebook, params.title)
    return opened(ctx, id, problem)
  end,

  save = function(ctx, params)
    local problem = store.save(params.note, params.body)
    return if problem then ctx:fail(problem) else store.library()
  end,

  rename = function(ctx, params)
    local problem = store.rename(params.note, params.title)
    return opened(ctx, params.note, problem)
  end,

  pin = function(ctx, params)
    local problem = store.pin(params.note, params.pinned)
    return if problem then ctx:fail(problem) else store.library()
  end,

  remove = function(ctx, params)
    local problem = store.remove(params.note)
    return if problem then ctx:fail(problem) else store.library()
  end,

  search = function(_ctx, params)
    return { hits = store.search(params.notebook, params.query) }
  end,

  addNotebook = function(ctx, params)
    local problem = store.addNotebook(params.name)
    return if problem then ctx:fail(problem) else store.library()
  end,

  removeNotebook = function(ctx, params)
    local problem = store.removeNotebook(params.notebook)
    return if problem then ctx:fail(problem) else store.library()
  end,
}

local NOTEBOOK = "The name of the notebook; the first notebook when left out"

-- The notebook an agent means: the one it names, or the first. Nothing, and why, when there is no notebook so called.
local function notebookOf(name: string?): (rpc.Notebook?, string?)
  if name == nil then
    return store.notebooks()[1], nil
  end
  local notebook = store.notebookNamed(name)
  if not notebook then
    return nil, `There is no notebook called {name}.`
  end
  return notebook, nil
end

app.tool {
  name = "list_notes",
  title = "List the notes",
  readOnly = true,
  description = "The notes of a notebook, the most recently changed first: each with its id, its title, its first line and its tags. Give `tag` for the notes with that tag only. The notebooks' names are listed too.",
  input = {
    notebook = t.string { optional = true, description = NOTEBOOK },
    tag = t.string { optional = true, description = "A tag, without the #" },
  },
  run = function(input: { notebook: string?, tag: string? }, ctx)
    local notebook, problem = notebookOf(input.notebook)
    if not notebook then
      return ctx:fail(problem or "There is no notebook.")
    end
    local tag = if input.tag ~= nil then string.lower((string.gsub(input.tag, "^#", ""))) else nil
    local notes: { { id: number, title: string, excerpt: string, tags: { string } } } = {}
    for _, note in store.summaries(notebook.id) do
      if tag == nil or table.find(note.tags, tag) ~= nil then
        table.insert(notes, { id = note.id, title = note.title, excerpt = note.excerpt, tags = note.tags })
      end
    end
    local names: { string } = {}
    for _, each in store.notebooks() do
      table.insert(names, each.name)
    end
    return { notebook = notebook.name, notes = notes, notebooks = names }
  end,
}

app.tool {
  name = "read_note",
  title = "Read a note",
  readOnly = true,
  description = "A note's Markdown text and the titles of the notes that link to it, by its title (whatever its capitals). In the text, [[Title]] is a link to the note with that title, and #tag is a tag.",
  input = {
    title = t.string { description = "The note's title" },
    notebook = t.string { optional = true, description = NOTEBOOK },
  },
  run = function(input: { title: string, notebook: string? }, ctx)
    local notebook, problem = notebookOf(input.notebook)
    if not notebook then
      return ctx:fail(problem or "There is no notebook.")
    end
    local id = store.noteNamed(notebook.id, input.title)
    local note = if id ~= nil then store.note(id) else nil
    if not note then
      return ctx:fail(`There is no note called {input.title} in {notebook.name}.`)
    end
    local linkedFrom: { string } = {}
    for _, backlink in note.backlinks do
      table.insert(linkedFrom, backlink.title)
    end
    return { id = note.id, title = note.title, notebook = notebook.name, body = note.body, linkedFrom = linkedFrom }
  end,
}

app.tool {
  name = "write_note",
  title = "Write a note",
  description = "Write a note in Markdown: a new note when the notebook has none with that title, otherwise the text replaces what the note says, or is added after it with `append`. Write [[Title]] to link to another note (it need not exist yet) and #tag to tag the note.",
  input = {
    title = t.string { description = "The note's title, without [ ] | # or ^", maxLength = 120 },
    body = t.string { description = "The Markdown text", maxLength = 200000 },
    notebook = t.string { optional = true, description = NOTEBOOK },
    append = t.boolean { optional = true, description = "True to add the text after what the note already says" },
  },
  run = function(input: { title: string, body: string, notebook: string?, append: boolean? }, ctx)
    local notebook, missing = notebookOf(input.notebook)
    if not notebook then
      return ctx:fail(missing or "There is no notebook.")
    end
    local id = store.noteNamed(notebook.id, input.title)
    local created = id == nil
    if id == nil then
      local made, problem = store.create(notebook.id, input.title, input.body)
      if made == nil then
        return ctx:fail(problem or "The note could not be made.")
      end
      id = made
    else
      local body = input.body
      local before = if input.append then store.note(id) else nil
      if before ~= nil and before.body ~= "" then
        body = before.body .. "\n\n" .. input.body
      end
      local problem = store.save(id, body)
      if problem then
        return ctx:fail(problem)
      end
    end
    app.emit("notebooks.changed")
    return { id = id, notebook = notebook.name, created = created }
  end,
}

app.tool {
  name = "search_notes",
  title = "Search the notes",
  readOnly = true,
  description = "The notes of a notebook with the words in their title or their text, each with the line the words are in.",
  input = {
    query = t.string { description = "The words to look for", maxLength = 200 },
    notebook = t.string { optional = true, description = NOTEBOOK },
  },
  run = function(input: { query: string, notebook: string? }, ctx)
    local notebook, problem = notebookOf(input.notebook)
    if not notebook then
      return ctx:fail(problem or "There is no notebook.")
    end
    return { notebook = notebook.name, notes = store.search(notebook.id, input.query) }
  end,
}

return handlers
