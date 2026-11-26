--!strict
-- The Notebooks app's backend: notebooks of Markdown notes that link to each other, in the app's own database
-- (store.luau). What a note's text says about it (its links, its tags) is read from the text here (text.luau), so a
-- note written by an agent is linked and tagged as one written in the window is. It serves on the RPC bus through the
-- global `RPC`, typed by the generated `types/vos.d.luau`.
--
-- A call that changes something answers with the library as it now stands (and with the note, when the call is about
-- one), so the window that asked draws what the backend has.
--
-- The page's methods are private (`_library`, `_read`, `_create`, ...). An agent can do what a person does most: see
-- the notes (`list_notes`), read one (`read_note`), write one (`write_note`) and look for words (`search_notes`). A tool
-- that changed something tells the open windows with the `notebooks_changed` notification, and they read the library
-- again.
local store = require("./store")

-- the note with this id and the library, for a call that made, found or renamed it
local function opened(ctx: vos_app_Context, id: number?, problem: string?): notebooks_Opened
  if problem ~= nil then
    return ctx:fail(problem)
  end
  local note = if id ~= nil then store.note(id) else nil
  if not note then
    return ctx:fail("There is no such note.")
  end
  return { library = store.library(), note = note }
end

RPC.on_request("_library", function(): notebooks_LibraryResult
  return store.library()
end)

RPC.on_request("_read", function(params: notebooks_ReadParams, ctx: vos_app_Context): notebooks_ReadResult
  local note = store.note(params.note)
  return if note then note else ctx:fail("There is no such note.")
end)

RPC.on_request("_create", function(params: notebooks_CreateParams, ctx: vos_app_Context): notebooks_CreateResult
  local id, problem = store.create(params.notebook, params.title, nil)
  return opened(ctx, id, problem)
end)

RPC.on_request("_follow", function(params: notebooks_FollowParams, ctx: vos_app_Context): notebooks_FollowResult
  local id, problem = store.follow(params.notebook, params.title)
  return opened(ctx, id, problem)
end)

RPC.on_request("_save", function(params: notebooks_SaveParams, ctx: vos_app_Context): notebooks_SaveResult
  local problem = store.save(params.note, params.body)
  return if problem then ctx:fail(problem) else store.library()
end)

RPC.on_request("_rename", function(params: notebooks_RenameParams, ctx: vos_app_Context): notebooks_RenameResult
  local problem = store.rename(params.note, params.title)
  return opened(ctx, params.note, problem)
end)

RPC.on_request("_pin", function(params: notebooks_PinParams, ctx: vos_app_Context): notebooks_PinResult
  local problem = store.pin(params.note, params.pinned)
  return if problem then ctx:fail(problem) else store.library()
end)

RPC.on_request("_remove", function(params: notebooks_RemoveParams, ctx: vos_app_Context): notebooks_RemoveResult
  local problem = store.remove(params.note)
  return if problem then ctx:fail(problem) else store.library()
end)

RPC.on_request("_search", function(params: notebooks_SearchParams): notebooks_SearchResult
  return { hits = store.search(params.notebook, params.query) }
end)

RPC.on_request(
  "_add_notebook",
  function(params: notebooks_AddNotebookParams, ctx: vos_app_Context): notebooks_AddNotebookResult
    local problem = store.addNotebook(params.name)
    return if problem then ctx:fail(problem) else store.library()
  end
)

RPC.on_request(
  "_remove_notebook",
  function(params: notebooks_RemoveNotebookParams, ctx: vos_app_Context): notebooks_RemoveNotebookResult
    local problem = store.removeNotebook(params.notebook)
    return if problem then ctx:fail(problem) else store.library()
  end
)

-- The notebook an agent means: the one it names, or the first. Nothing, and why, when there is no notebook so called.
local function notebookOf(name: string?): (notebooks_Notebook?, string?)
  if name == nil then
    return store.notebooks()[1], nil
  end
  local notebook = store.notebookNamed(name)
  if not notebook then
    return nil, `There is no notebook called {name}.`
  end
  return notebook, nil
end

RPC.on_request(
  "list_notes",
  function(params: notebooks_ListNotesParams, ctx: vos_app_Context): notebooks_ListNotesResult
    local notebook, problem = notebookOf(params.notebook)
    if not notebook then
      return ctx:fail(problem or "There is no notebook.")
    end
    local tag = if params.tag ~= nil then string.lower((string.gsub(params.tag, "^#", ""))) else nil
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
  end
)

RPC.on_request("read_note", function(params: notebooks_ReadNoteParams, ctx: vos_app_Context): notebooks_ReadNoteResult
  local notebook, problem = notebookOf(params.notebook)
  if not notebook then
    return ctx:fail(problem or "There is no notebook.")
  end
  local id = store.noteNamed(notebook.id, params.title)
  local note = if id ~= nil then store.note(id) else nil
  if not note then
    return ctx:fail(`There is no note called {params.title} in {notebook.name}.`)
  end
  local linkedFrom: { string } = {}
  for _, backlink in note.backlinks do
    table.insert(linkedFrom, backlink.title)
  end
  return { id = note.id, title = note.title, notebook = notebook.name, body = note.body, linkedFrom = linkedFrom }
end)

RPC.on_request(
  "write_note",
  function(params: notebooks_WriteNoteParams, ctx: vos_app_Context): notebooks_WriteNoteResult
    local notebook, missing = notebookOf(params.notebook)
    if not notebook then
      return ctx:fail(missing or "There is no notebook.")
    end
    local found = store.noteNamed(notebook.id, params.title)
    local created = found == nil
    local id: number
    if found == nil then
      local made, problem = store.create(notebook.id, params.title, params.body)
      if made == nil then
        return ctx:fail(problem or "The note could not be made.")
      end
      id = made
    else
      id = found
      local body = params.body
      local before = if params.append then store.note(id) else nil
      if before ~= nil and before.body ~= "" then
        body = before.body .. "\n\n" .. params.body
      end
      local problem = store.save(id, body)
      if problem then
        return ctx:fail(problem)
      end
    end
    RPC.send_notification("notebooks_changed")
    return { id = id, notebook = notebook.name, created = created }
  end
)

RPC.on_request(
  "search_notes",
  function(params: notebooks_SearchNotesParams, ctx: vos_app_Context): notebooks_SearchNotesResult
    local notebook, problem = notebookOf(params.notebook)
    if not notebook then
      return ctx:fail(problem or "There is no notebook.")
    end
    return { notebook = notebook.name, notes = store.search(notebook.id, params.query) }
  end
)
