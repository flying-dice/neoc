--!strict
-- The Notes app's backend: folders of notes, in the app's own database (store.luau). Every call that changes something
-- answers with the whole board as it now stands, so the window that asked draws what the backend has. A board lists a
-- note by its title and a line of what follows; `_read` gives the whole text of one. It serves on the RPC bus through
-- the global `RPC`, typed by the generated `types/vos.d.luau`.
--
-- The page's methods are private (`_board`, `_add_folder`, ...). An agent can do what a person does most: find notes
-- (`list_notes`), read one (`read_note`), write a new one (`create_note`) and add to one (`append_to_note`). A tool that
-- changed something tells the open windows with the `notes_changed` notification, and they read the board again.
local store = require("./store")

RPC.on_request("_board", function(): notes_BoardResult
  store.sweep()
  return store.board()
end)

RPC.on_request("_add_folder", function(params: notes_AddFolderParams, ctx: vos_app_Context): notes_AddFolderResult
  local problem = store.addFolder(params.name)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request(
  "_remove_folder",
  function(params: notes_RemoveFolderParams, ctx: vos_app_Context): notes_RemoveFolderResult
    local problem = store.removeFolder(params.folder)
    return if problem then ctx:fail(problem) else store.board()
  end
)

RPC.on_request("_create", function(params: notes_CreateParams, ctx: vos_app_Context): notes_CreateResult
  local _id, problem = store.create(params.folder)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request("_read", function(params: notes_ReadParams, ctx: vos_app_Context): notes_ReadResult
  local body = store.body(params.note)
  if body == nil then
    return ctx:fail("There is no such note.")
  end
  return { note = params.note, body = body }
end)

RPC.on_request("_write", function(params: notes_WriteParams, ctx: vos_app_Context): notes_WriteResult
  local problem = store.write(params.note, params.body)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request("_pin", function(params: notes_PinParams, ctx: vos_app_Context): notes_PinResult
  local problem = store.pin(params.note, params.pinned)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request("_move", function(params: notes_MoveParams, ctx: vos_app_Context): notes_MoveResult
  local problem = store.move(params.note, params.folder)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request("_remove", function(params: notes_RemoveParams, ctx: vos_app_Context): notes_RemoveResult
  local problem = store.remove(params.note)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request("_restore", function(params: notes_RestoreParams, ctx: vos_app_Context): notes_RestoreResult
  local problem = store.restore(params.note)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request("_purge", function(params: notes_PurgeParams, ctx: vos_app_Context): notes_PurgeResult
  local problem = store.purge(params.note)
  return if problem then ctx:fail(problem) else store.board()
end)

RPC.on_request("_search", function(params: notes_SearchParams): notes_SearchResult
  return { notes = store.search(params.query) }
end)

RPC.on_request("list_notes", function(params: notes_ListNotesParams, ctx: vos_app_Context): notes_ListNotesResult
  local names: { [number]: string } = {}
  for _, folder in store.folders() do
    names[folder.id] = folder.name
  end
  local only: number? = nil
  if params.folder ~= nil then
    local folder = store.folderNamed(params.folder)
    if not folder then
      return ctx:fail(`There is no folder called {params.folder}.`)
    end
    only = folder.id
  end
  local found: { [number]: boolean }? = nil
  if params.search ~= nil and params.search ~= "" then
    local ids: { [number]: boolean } = {}
    for _, id in store.search(params.search) do
      ids[id] = true
    end
    found = ids
  end
  local notes: { notes_Listed } = {}
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
end)

RPC.on_request("read_note", function(params: notes_ReadNoteParams, ctx: vos_app_Context): notes_ReadNoteResult
  local note, body = store.note(params.id), store.body(params.id)
  if not note or body == nil or note.deleted then
    return ctx:fail("There is no such note.")
  end
  return { id = note.id, title = note.title, body = body, pinned = note.pinned, edited = note.edited }
end)

RPC.on_request("create_note", function(params: notes_CreateNoteParams, ctx: vos_app_Context): notes_CreateNoteResult
  local folder = if params.folder ~= nil then store.folderNamed(params.folder) else store.folders()[1]
  if not folder then
    return ctx:fail(`There is no folder called {params.folder or ""}.`)
  elseif string.match(params.body, "%S") == nil then
    return ctx:fail("A note needs some text.")
  end
  local id, problem = store.create(folder.id, params.body)
  if problem then
    return ctx:fail(problem)
  end
  RPC.send_notification("notes_changed")
  return { id = id :: number, folder = folder.name }
end)

RPC.on_request(
  "append_to_note",
  function(params: notes_AppendToNoteParams, ctx: vos_app_Context): notes_AppendToNoteResult
    local body = store.body(params.id)
    if body == nil then
      return ctx:fail("There is no such note.")
    end
    local joined = if body == "" or string.sub(body, -1) == "\n"
      then body .. params.text
      else body .. "\n" .. params.text
    local problem = store.write(params.id, joined)
    if problem then
      return ctx:fail(problem)
    end
    RPC.send_notification("notes_changed")
    local note = store.note(params.id)
    return { id = params.id, title = if note then note.title else "" }
  end
)
