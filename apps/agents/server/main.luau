--!strict
-- The Agents app's backend: the chats, what was said in them, and the agent's turn. It is self-contained: the chats and
-- their messages are in the app's own database, and the agent is this backend's own (agent.luau, a mock for now).
--
-- A turn: `send` stores what the person said, answers at once, and leaves a task running that asks the agent. Each piece
-- of the agent's answer is added to the stored message and sent to the open window as a `piece` event; a `turn` event
-- says the answer is over (complete, stopped or failed). A window that missed events reads the chat again with `open`.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local task = require("std:task")
local agent = require("./agent")
local db = app.db

db:migrate {
  [1] = [[
    create table threads (
      id text primary key default (lower(hex(randomblob(8)))),
      agent_id text not null,
      agent_name text not null,
      title text not null default 'New chat',
      updated_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    create index threads_by_use on threads(updated_at desc, id desc);
  ]],
  [2] = [[
    create table messages (
      id integer primary key,
      thread_id text not null,
      role text not null,
      text text not null default '',
      done integer not null default 1
    );
    create index messages_by_thread on messages(thread_id, id);
  ]],
}

-- no turn outlives the backend: an answer that was still coming when it last stopped is as long as it will get
db:exec("update messages set done = 1 where done = 0")

-- the turn in progress in each chat: the task asking the agent, and the message its answer goes into
type Turn = { task: any, message: number }
local running: { [string]: Turn } = {}

local TITLE_MAX = 48

-- the first message names the chat; later messages only move it to the top
local function titleOf(text: string): string
  local line = string.gsub(string.match(text, "^%s*([^\n]*)") or "", "%s+$", "")
  local length = utf8.len(line)
  if length == nil or length <= TITLE_MAX then
    return if line == "" then "New chat" else line
  end
  return string.sub(line, 1, utf8.offset(line, TITLE_MAX) - 1) .. "…"
end

local function chatOf(row: { [string]: any }): rpc.Chat
  return { id = row.id, title = row.title, agent = row.agent_name, updated = row.updated_at }
end

local function chats(): { rpc.Chat }
  local list: { rpc.Chat } = {}
  for _, row in
    db:all("select id, title, agent_name, updated_at from threads order by updated_at desc, id desc limit 200")
  do
    table.insert(list, chatOf(row))
  end
  return list
end

local function conversation(id: string): rpc.Conversation?
  local row = db:one("select id, title, agent_name, updated_at from threads where id = :id", { id = id })
  if not row then
    return nil
  end
  local messages: { rpc.Message } = {}
  for _, m in db:all("select id, role, text, done from messages where thread_id = :id order by id", { id = id }) do
    table.insert(messages, { id = m.id, chat = id, role = m.role, text = m.text, done = m.done == 1 })
  end
  return { chat = chatOf(row), messages = messages, busy = running[id] ~= nil }
end

-- the turn in `chat` is over, however it ended
local function finish(chat: string, turn: Turn)
  db:exec("update messages set done = 1 where id = :id", { id = turn.message })
  if running[chat] == turn then
    running[chat] = nil
  end
  app.emit("turn", { chat = chat })
end

local handlers: rpc.Handlers = {
  agents = function(_ctx)
    return agent.roster()
  end,

  chats = function(_ctx)
    return chats()
  end,

  start = function(ctx, params)
    local who = agent.find(params.agent)
    if not who then
      return ctx:fail("There is no such agent.")
    end
    local made = db:exec("insert into threads(agent_id, agent_name) values (:a, :n)", { a = who.id, n = who.name })
    local row = db:one("select id from threads where rowid = :r", { r = made.last_insert_id })
    return conversation((row :: { [string]: any }).id) :: rpc.Conversation
  end,

  open = function(ctx, params)
    return conversation(params.chat) or ctx:fail("There is no such chat.")
  end,

  send = function(ctx, params)
    local chat = params.chat
    local row = db:one("select agent_id from threads where id = :id", { id = chat })
    if not row then
      return ctx:fail("There is no such chat.")
    end
    local text = string.match(params.text, "^%s*(.-)%s*$") :: string
    if text == "" then
      return ctx:fail("Write something first.")
    end
    if running[chat] then
      return ctx:fail("The agent is still answering.")
    end
    local who = agent.find(row.agent_id)
    if not who then
      return ctx:fail("That agent is not here any more.")
    end

    local answer = db:tx(function(): number
      db:exec("insert into messages(thread_id, role, text) values (:c, 'user', :t)", { c = chat, t = text })
      db:exec(
        [[update threads
          set title = case when title = 'New chat' then :t else title end,
              updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          where id = :id]],
        { t = titleOf(text), id = chat }
      )
      return db:exec("insert into messages(thread_id, role, done) values (:c, 'agent', 0)", { c = chat }).last_insert_id
    end)

    local turn: Turn = { task = nil, message = answer }
    running[chat] = turn
    -- deferred: the turn is recorded (and this call has answered) before the agent says anything
    turn.task = task.defer(function()
      local ok, problem = pcall(function(): string?
        agent.prompt(who, text, function(piece: string)
          db:exec("update messages set text = text || :p where id = :id", { p = piece, id = answer })
          app.emit("piece", { chat = chat, message = answer, text = piece })
        end)
        return nil
      end)
      if not ok then
        db:exec(
          "update messages set text = text || :p where id = :id",
          { p = `\n\nThe agent stopped: {tostring(problem)}`, id = answer }
        )
      end
      finish(chat, turn)
    end)

    return conversation(chat) :: rpc.Conversation
  end,

  stop = function(ctx, params)
    local turn = running[params.chat]
    if turn then
      -- a cancelled task does not run again, so its own `finish` never happens: finish for it
      task.cancel(turn.task)
      finish(params.chat, turn)
    end
    return conversation(params.chat) or ctx:fail("There is no such chat.")
  end,
}

return handlers
