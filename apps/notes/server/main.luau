--!strict
-- The Notes app's backend: folders of notes, in the app's own database (store.luau). Every call that changes something
-- answers with the whole board as it now stands, so the window that asked draws what the backend has. A board lists a
-- note by its title and a line of what follows; `read` gives the whole text of one.
--
-- An agent can do what a person does most: find notes (`list_notes`), read one (`read_note`), write a new one
-- (`create_note`) and add to one (`append_to_note`). A tool that changed something tells the open windows with a
-- `notes.changed` event, and they read the board again.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local store = require("./store")
local t = app.t

local handlers: rpc.Handlers = {
  board = function(_ctx)
    store.sweep()
    return store.board()
  end,

  addFolder = function(ctx, params)
    local problem = store.addFolder(params.name)
    return if problem then ctx:fail(problem) else store.board()
  end,

  removeFolder = function(ctx, params)
    local problem = store.removeFolder(params.folder)
    return if problem then ctx:fail(problem) else store.board()
  end,

  create = function(ctx, params)
    local _id, problem = store.create(params.folder)
    return if problem then ctx:fail(problem) else store.board()
  end,

  read = function(ctx, params)
    local body = store.body(params.note)
    if body == nil then
      return ctx:fail("There is no such note.")
    end
    return { note = params.note, body = body }
  end,

  write = function(ctx, params)
    local problem = store.write(params.note, params.body)
    return if problem then ctx:fail(problem) else store.board()
  end,

  pin = function(ctx, params)
    local problem = store.pin(params.note, params.pinned)
    return if problem then ctx:fail(problem) else store.board()
  end,

  move = function(ctx, params)
    local problem = store.move(params.note, params.folder)
    return if problem then ctx:fail(problem) else store.board()
  end,

  remove = function(ctx, params)
    local problem = store.remove(params.note)
    return if problem then ctx:fail(problem) else store.board()
  end,

  restore = function(ctx, params)
    local problem = store.restore(params.note)
    return if problem then ctx:fail(problem) else store.board()
  end,

  purge = function(ctx, params)
    local problem = store.purge(params.note)
    return if problem then ctx:fail(problem) else store.board()
  end,

  search = function(_ctx, params)
    return { notes = store.search(params.query) }
  end,
}

-- a note as an agent reads it in a list: its folder by name
type Listed = { id: number, title: string, preview: string, folder: string, pinned: boolean, edited: string }

app.tool {
  name = "list_notes",
  title = "List the notes",
  readOnly = true,
  description = "The notes, the most recently edited first: each with its id, its title (its first line), the line after it, the folder it is in, whether it is pinned and when it was last edited. Give `folder` for one folder only, and `search` for the notes whose text holds some words. Deleted notes are left out. `read_note` gives the whole text of one.",
  input = {
    folder = t.string { optional = true, description = "The name of a folder; every folder when left out" },
    search = t.string { optional = true, description = "Words the note's text must hold", maxLength = 100 },
  },
  run = function(input: { folder: string?, search: string? }, ctx)
    local names: { [number]: string } = {}
    for _, folder in store.folders() do
      names[folder.id] = folder.name
    end
    local only: number? = nil
    if input.folder ~= nil then
      local folder = store.folderNamed(input.folder)
      if not folder then
        return ctx:fail(`There is no folder called {input.folder}.`)
      end
      only = folder.id
    end
    local found: { [number]: boolean }? = nil
    if input.search ~= nil and input.search ~= "" then
      local ids: { [number]: boolean } = {}
      for _, id in store.search(input.search) do
        ids[id] = true
      end
      found = ids
    end
    local notes: { Listed } = {}
    for _, note in store.notes() do
      if not note.deleted and (only == nil or note.folder == only) and (found == nil or found[note.id]) then
        table.insert(notes, {
          id = note.id,
          title = note.title,
          preview = note.preview,
          folder = names[note.folder] or "",
          pinned = note.pinned,
          edited = note.edited,
        })
      end
    end
    return { notes = notes }
  end,
}

app.tool {
  name = "read_note",
  title = "Read a note",
  readOnly = true,
  description = "The whole text of a note, by its id (`list_notes` gives the ids).",
  input = { id = t.integer { description = "The id of the note" } },
  run = function(input: { id: number }, ctx)
    local note, body = store.note(input.id), store.body(input.id)
    if not note or body == nil or note.deleted then
      return ctx:fail("There is no such note.")
    end
    return { id = note.id, title = note.title, body = body, pinned = note.pinned, edited = note.edited }
  end,
}

app.tool {
  name = "create_note",
  title = "Write a note",
  description = "Write a new note in a folder (the first folder when none is named). Its first line is its title.",
  input = {
    body = t.string { description = "The text of the note; its first line is its title", maxLength = store.MAX_BODY },
    folder = t.string {
      optional = true,
      description = "The name of the folder to put it in; the first folder when left out",
    },
  },
  run = function(input: { body: string, folder: string? }, ctx)
    local folder = if input.folder ~= nil then store.folderNamed(input.folder) else store.folders()[1]
    if not folder then
      return ctx:fail(`There is no folder called {input.folder or ""}.`)
    elseif string.match(input.body, "%S") == nil then
      return ctx:fail("A note needs some text.")
    end
    local id, problem = store.create(folder.id, input.body)
    if problem then
      return ctx:fail(problem)
    end
    app.emit("notes.changed")
    return { id = id, folder = folder.name }
  end,
}

app.tool {
  name = "append_to_note",
  title = "Add to a note",
  description = "Add text to the end of a note, on a line of its own, by the note's id (`list_notes` gives the ids).",
  input = {
    id = t.integer { description = "The id of the note" },
    text = t.string { description = "What to add", maxLength = store.MAX_BODY },
  },
  run = function(input: { id: number, text: string }, ctx)
    local body = store.body(input.id)
    if body == nil then
      return ctx:fail("There is no such note.")
    end
    local joined = if body == "" or string.sub(body, -1) == "\n" then body .. input.text else body .. "\n" .. input.text
    local problem = store.write(input.id, joined)
    if problem then
      return ctx:fail(problem)
    end
    app.emit("notes.changed")
    local note = store.note(input.id)
    return { id = input.id, title = if note then note.title else "" }
  end,
}

return handlers
