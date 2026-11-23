--!strict
-- What was said and done in a chat, in order: the items. Each change is written to the database and sent to the open
-- window: a whole item as an `item` event, a piece of text added to one as a `piece` event (an answer arrives in
-- hundreds of pieces, and the item is not sent again for each).
local app = require("vos:app")
local rpc = require("../.app/rpc")
local db = app.db

export type Kind =
  "user"
  | "assistant"
  | "reasoning"
  | "tool"
  | "activity"
  | "step"
  | "subagent"
  | "custom"
  | "interrupt"
  | "error"
-- An item as the database has it: `key` is the id the agent gave the thing (a message id, a tool call id), by which a
-- later event finds it.
export type Row = { id: number, key: string, kind: Kind, text: string, name: string, data: string, done: boolean }
export type Fields = { key: string?, text: string?, name: string?, data: string?, done: boolean? }

local timeline = {}

local function rowOf(r: { [string]: any }): Row
  return { id = r.id, key = r.key, kind = r.kind, text = r.text, name = r.name, data = r.data, done = r.done == 1 }
end

local function itemOf(chat: string, r: Row): rpc.Item
  return { id = r.id, chat = chat, kind = r.kind, text = r.text, name = r.name, data = r.data, done = r.done }
end

local COLUMNS = "id, key, kind, text, name, data, done"

function timeline.rows(chat: string): { Row }
  local list: { Row } = {}
  for _, r in db:all(`select {COLUMNS} from items where thread_id = :c order by id`, { c = chat }) do
    table.insert(list, rowOf(r))
  end
  return list
end

function timeline.items(chat: string): { rpc.Item }
  local list: { rpc.Item } = {}
  for _, row in timeline.rows(chat) do
    table.insert(list, itemOf(chat, row))
  end
  return list
end

function timeline.get(chat: string, id: number): Row?
  local r = db:one(`select {COLUMNS} from items where thread_id = :c and id = :id`, { c = chat, id = id })
  return if r then rowOf(r) else nil
end

-- the item of that kind the agent named `key`, in this chat
function timeline.find(chat: string, kind: Kind, key: string): Row?
  local r = db:one(
    `select {COLUMNS} from items where thread_id = :c and kind = :k and key = :key order by id desc limit 1`,
    { c = chat, k = kind, key = key }
  )
  return if r then rowOf(r) else nil
end

local function announce(chat: string, id: number)
  local row = timeline.get(chat, id)
  if row then
    app.emit("item", { chat = chat, item = itemOf(chat, row) })
  end
end

-- Add an item at the end of the chat and tell the window. Returns its id.
function timeline.add(chat: string, kind: Kind, fields: Fields): number
  local id = db:exec(
    [[insert into items(thread_id, kind, key, text, name, data, done)
      values (:c, :kind, :key, :text, :name, :data, :done)]],
    {
      c = chat,
      kind = kind,
      key = fields.key or "",
      text = fields.text or "",
      name = fields.name or "",
      data = fields.data or "",
      done = if fields.done == false then 0 else 1,
    }
  ).last_insert_id
  announce(chat, id)
  return id
end

-- Change the fields given of an item and tell the window.
function timeline.set(chat: string, id: number, fields: Fields)
  db:exec(
    [[update items set
        text = coalesce(:text, text), name = coalesce(:name, name), data = coalesce(:data, data),
        done = coalesce(:done, done)
      where thread_id = :c and id = :id]],
    {
      c = chat,
      id = id,
      text = fields.text,
      name = fields.name,
      data = fields.data,
      done = if fields.done == nil then nil elseif fields.done then 1 else 0,
    }
  )
  announce(chat, id)
end

-- Add a piece to an item's text and send the window the piece.
function timeline.append(chat: string, id: number, piece: string)
  if piece == "" then
    return
  end
  db:exec("update items set text = text || :p where thread_id = :c and id = :id", { p = piece, c = chat, id = id })
  app.emit("piece", { chat = chat, item = id, text = piece })
end

-- Nothing in the chat is still coming: a run that ended, however it ended, leaves no spinner behind.
function timeline.settle(chat: string)
  for _, r in db:all("select id from items where thread_id = :c and done = 0 and kind <> 'interrupt'", { c = chat }) do
    timeline.set(chat, r.id, { done = true })
  end
end

return timeline
