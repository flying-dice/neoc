--!strict
-- What was said and done in a chat, in order: the items. Each change is written to the database and sent to the open
-- window: a whole item as an `item` notification, a piece of text added to one as a `piece` notification (an answer arrives in
-- hundreds of pieces, and the item is not sent again for each). A notification is a hint that the chat changed, and the
-- host takes at most 64 KiB of one: what does not fit (a tool's result, say) is sent as an `item_changed` notification,
-- which names the item, and the window reads it (`_read_item`).
local codec = require("./codec")
local db = app.db

export type Kind = "user" | "assistant" | "reasoning" | "tool" | "plan" | "permission" | "note" | "error"
-- An item as the database has it: `key` is the id the agent gave the thing (a tool call id, a permission request's
-- id), by which a later update finds it.
export type Row = { id: number, key: string, kind: Kind, text: string, name: string, data: string, done: boolean }
export type Fields = { key: string?, text: string?, name: string?, data: string?, done: boolean? }

-- The most bytes of JSON one notification of ours takes. The host refuses more than 64 KiB; the rest is left for what
-- the host adds, and for the difference between the two encoders.
local MAX_NOTIFICATION = 60 * 1024

local timeline = {}

local function rowOf(r: { [string]: any }): Row
  return { id = r.id, key = r.key, kind = r.kind, text = r.text, name = r.name, data = r.data, done = r.done == 1 }
end

local function itemOf(chat: string, r: Row): agents_Item
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

function timeline.items(chat: string): { agents_Item }
  local list: { agents_Item } = {}
  for _, row in timeline.rows(chat) do
    table.insert(list, itemOf(chat, row))
  end
  return list
end

function timeline.get(chat: string, id: number): Row?
  local r = db:one(`select {COLUMNS} from items where thread_id = :c and id = :id`, { c = chat, id = id })
  return if r then rowOf(r) else nil
end

-- an item as the window has it
function timeline.item(chat: string, id: number): agents_Item?
  local row = timeline.get(chat, id)
  return if row then itemOf(chat, row) else nil
end

-- The item of that kind the agent named `key`, in this chat. With `after`, only an item added after the item of that
-- id: an agent may use the same id again in a later turn (`call_1`), and it means a new thing by it.
function timeline.find(chat: string, kind: Kind, key: string, after: number?): Row?
  local r = db:one(
    `select {COLUMNS} from items where thread_id = :c and kind = :k and key = :key and id > :after`
      .. " order by id desc limit 1",
    { c = chat, k = kind, key = key, after = after or 0 }
  )
  return if r then rowOf(r) else nil
end

-- the id of the last item of the chat, 0 when it has none
function timeline.last(chat: string): number
  local r = db:one("select coalesce(max(id), 0) as id from items where thread_id = :c", { c = chat })
  return if r then r.id else 0
end

-- Whether the host takes `params` as a notification's: it refuses more than 64 KiB of JSON.
local function fits(params: { [string]: any }): boolean
  return #codec.encode(params) <= MAX_NOTIFICATION
end

-- Tell the window the item `id` of `chat` changed, and was too large to send: it reads the item.
local function changed(chat: string, id: number)
  RPC.send_notification("_item_changed", { chat = chat, item = id })
end

local function announce(chat: string, id: number)
  local row = timeline.get(chat, id)
  if not row then
    return
  end
  -- an item whose text and data alone are over the limit does not fit: no need to encode it to learn that
  local params = { chat = chat, item = itemOf(chat, row) }
  if #row.text + #row.data <= MAX_NOTIFICATION and fits(params) then
    RPC.send_notification("_item", params)
  else
    changed(chat, id)
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
  -- only the columns given are named: a parameter with no value is no parameter
  local changes: { string } = {}
  local values: { [string]: any } = { c = chat, id = id }
  if fields.text ~= nil then
    table.insert(changes, "text = :text")
    values.text = fields.text
  end
  if fields.name ~= nil then
    table.insert(changes, "name = :name")
    values.name = fields.name
  end
  if fields.data ~= nil then
    table.insert(changes, "data = :data")
    values.data = fields.data
  end
  if fields.done ~= nil then
    table.insert(changes, "done = :done")
    values.done = if fields.done then 1 else 0
  end
  if #changes > 0 then
    db:exec(`update items set {table.concat(changes, ", ")} where thread_id = :c and id = :id`, values)
  end
  announce(chat, id)
end

-- Add a piece to an item's text and send the window the piece.
function timeline.append(chat: string, id: number, piece: string)
  if piece == "" then
    return
  end
  db:exec("update items set text = text || :p where thread_id = :c and id = :id", { p = piece, c = chat, id = id })
  local params = { chat = chat, item = id, text = piece }
  if fits(params) then
    RPC.send_notification("_piece", params)
  else
    changed(chat, id)
  end
end

-- Nothing in the chat is still coming: a turn that ended, however it ended, leaves no spinner behind, and no
-- question of the agent's open (it asked within the turn, and the turn is over).
function timeline.settle(chat: string)
  for _, r in db:all("select id from items where thread_id = :c and done = 0", { c = chat }) do
    timeline.set(chat, r.id, { done = true })
  end
end

return timeline
