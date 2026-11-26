--!strict
-- The Agents app's backend: the agents the person added (roster.luau), the chats with them, and the agent's turn in a
-- chat (turn.luau). An agent is an AG-UI endpoint: this backend posts the conversation to it and reads the events it
-- answers with (`lib:agui`). Everything is the app's own: the agents, chats and items are in its database, and a token
-- is in the keychain.
--
-- It serves on the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`. The page's methods are
-- private (`_agents`, `_send`, ...); the app has no tools of its own, and offers its agent the tools of the apps that
-- are running (`app.tools`, in turn.luau).
--
-- A turn: `_send` (or `_answer`, or `_act`) stores what the person did, answers at once, and leaves a task running the
-- agent's turn. What the agent says and does reaches the open window as `item` notifications (a whole item, new or
-- changed), `piece` notifications (text added to an item) and `state` notifications; a `turn` notification says the
-- turn is over (complete, stopped or failed). A window that missed some reads the chat again with `_open`.
local codec = require("./codec")
local roster = require("./roster")
local strings = require("lib:text")
local task = require("std:task")
local timeline = require("./timeline")
local turn = require("./turn")
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
  -- AG-UI agents replace the built-in one: its chats were with an agent that is gone, and go with it
  [3] = [[
    drop table messages;
    delete from threads;
    alter table threads add column state text not null default '';
    create table agents (
      id text primary key,
      name text not null,
      url text not null,
      has_token integer not null default 0
    );
    create table items (
      id integer primary key,
      thread_id text not null,
      kind text not null,
      key text not null default '',
      name text not null default '',
      text text not null default '',
      data text not null default '',
      done integer not null default 1
    );
    create index items_by_thread on items(thread_id, id);
    create index items_by_key on items(thread_id, kind, key);
  ]],
}

-- no turn outlives the backend: what was still coming when it last stopped is as long as it will get
db:exec("update items set done = 1 where done = 0 and kind <> 'interrupt'")

-- the turn in progress in each chat, and the task running it
type Running = { task: any, turn: turn.Turn }
local running: { [string]: Running } = {}

local TITLE_MAX = 48

-- the first message names the chat; later messages only move it to the top
local function titleOf(text: string): string
  local line = strings.trim(string.match(strings.trim_start(text), "^[^\n]*") or "")
  return if line == "" then "New chat" else strings.clip(line, TITLE_MAX, "…")
end

local function chatOf(row: { [string]: any }): agents_Chat
  return { id = row.id, title = row.title, agent = row.agent_name, updated = row.updated_at }
end

local function chats(): { agents_Chat }
  local list: { agents_Chat } = {}
  for _, row in
    db:all("select id, title, agent_name, updated_at from threads order by updated_at desc, id desc limit 200")
  do
    table.insert(list, chatOf(row))
  end
  return list
end

-- the questions of the agent's (interrupts) the chat still has to send it answers to
local function questions(chat: string): { timeline.Row }
  local list: { timeline.Row } = {}
  for _, row in timeline.rows(chat) do
    if row.kind == "interrupt" and not row.done then
      table.insert(list, row)
    end
  end
  return list
end

local function conversation(id: string): agents_Conversation?
  local row = db:one("select id, title, agent_name, updated_at, state from threads where id = :id", { id = id })
  if not row then
    return nil
  end
  return {
    chat = chatOf(row),
    items = timeline.items(id),
    busy = running[id] ~= nil,
    waiting = #questions(id) > 0,
    state = row.state,
  }
end

local function touch(chat: string, title: string?)
  db:exec(
    [[update threads
      set title = case when title = 'New chat' and :t <> '' then :t else title end,
          updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
      where id = :id]],
    { t = title or "", id = chat }
  )
end

-- the turn in `chat` is over, however it ended
local function finish(chat: string, mine: Running)
  timeline.settle(chat)
  if running[chat] == mine then
    running[chat] = nil
  end
  RPC.send_notification("_turn", { chat = chat })
end

-- Leave the agent's turn running in `chat`. Deferred: the caller has answered before the agent says anything.
local function begin(chat: string, agent: roster.Agent, extra: turn.Extra)
  local mine: Running = { task = nil, turn = { stream = nil, stopped = false } }
  running[chat] = mine
  mine.task = task.defer(function()
    local ok, problem = pcall(function(): string?
      turn.take(chat, agent, extra, mine.turn)
      return nil
    end)
    if not ok then
      timeline.add(chat, "error", { text = `The agent's turn stopped: {tostring(problem)}` })
    end
    finish(chat, mine)
  end)
end

-- The chat can take a turn now: it exists, its agent does, and no turn is running. Otherwise the reason, in the
-- person's words.
local function ready(chat: string): (roster.Agent?, string?)
  local row = db:one("select agent_id from threads where id = :id", { id = chat })
  if not row then
    return nil, "There is no such chat."
  end
  if running[chat] then
    return nil, "The agent is still answering."
  end
  local agent = roster.find(row.agent_id)
  if not agent then
    return nil, "That agent was removed. Its chats can be read, but not continued."
  end
  return agent, nil
end

RPC.on_request("_agents", function(): agents_AgentsResult
  return roster.all()
end)

RPC.on_request("_add_agent", function(params: agents_AddAgentParams, ctx: vos_app_Context): agents_AddAgentResult
  local problem = roster.add(params.name, params.url, params.token)
  if problem then
    return ctx:fail(problem)
  end
  return roster.all()
end)

RPC.on_request(
  "_update_agent",
  function(params: agents_UpdateAgentParams, ctx: vos_app_Context): agents_UpdateAgentResult
    local agent = roster.find(params.agent)
    if not agent then
      return ctx:fail("There is no such agent.")
    end
    local problem = roster.update(agent, params.name, params.url, params.token, params.forget == true)
    if problem then
      return ctx:fail(problem)
    end
    return roster.all()
  end
)

RPC.on_request(
  "_remove_agent",
  function(params: agents_RemoveAgentParams, ctx: vos_app_Context): agents_RemoveAgentResult
    local agent = roster.find(params.agent)
    if not agent then
      return ctx:fail("There is no such agent.")
    end
    roster.remove(agent)
    return roster.all()
  end
)

RPC.on_request("_chats", function(): agents_ChatsResult
  return chats()
end)

RPC.on_request("_start", function(params: agents_StartParams, ctx: vos_app_Context): agents_StartResult
  local agent = roster.find(params.agent)
  if not agent then
    return ctx:fail("There is no such agent.")
  end
  local made = db:exec("insert into threads(agent_id, agent_name) values (:a, :n)", { a = agent.id, n = agent.name })
  local row = db:one("select id from threads where rowid = :r", { r = made.last_insert_id })
  return conversation((row :: { [string]: any }).id) :: agents_Conversation
end)

RPC.on_request("_open", function(params: agents_OpenParams, ctx: vos_app_Context): agents_OpenResult
  return conversation(params.chat) or ctx:fail("There is no such chat.")
end)

RPC.on_request("_send", function(params: agents_SendParams, ctx: vos_app_Context): agents_SendResult
  local chat = params.chat
  local agent, problem = ready(chat)
  if not agent then
    return ctx:fail(problem or "")
  end
  local text = strings.trim(params.text)
  if text == "" then
    return ctx:fail("Write something first.")
  end
  if #questions(chat) > 0 then
    return ctx:fail("The agent asked something: answer or decline it first.")
  end
  timeline.add(chat, "user", { text = text })
  touch(chat, titleOf(text))
  begin(chat, agent, { resume = nil, forwarded = nil })
  return conversation(chat) :: agents_Conversation
end)

RPC.on_request("_answer", function(params: agents_AnswerParams, ctx: vos_app_Context): agents_AnswerResult
  local chat = params.chat
  local agent, problem = ready(chat)
  if not agent then
    return ctx:fail(problem or "")
  end
  local row = timeline.get(chat, params.item)
  if not row or row.kind ~= "interrupt" or row.done then
    return ctx:fail("That question is not open any more.")
  end
  local payload: any = nil
  if params.payload ~= nil and not params.decline then
    payload = codec.decode(params.payload)
    if payload == nil and string.match(params.payload, "^%s*null%s*$") == nil then
      return ctx:fail("The answer is not JSON.")
    end
  end
  local data = codec.decode(row.data) or {}
  data.status = if params.decline then "cancelled" else "resolved"
  data.payload = payload
  timeline.set(chat, row.id, { data = codec.encode(data) })

  -- the agent goes on when every question it stopped at has its answer: they go back to it together
  local open = questions(chat)
  local resume: { any } = {}
  for _, question in open do
    local answered = codec.decode(question.data) or {}
    if answered.status ~= "resolved" and answered.status ~= "cancelled" then
      return conversation(chat) :: agents_Conversation
    end
    table.insert(resume, { interrupt_id = question.key, status = answered.status, payload = answered.payload })
  end
  for _, question in open do
    timeline.set(chat, question.id, { done = true })
  end
  touch(chat, nil)
  begin(chat, agent, { resume = resume, forwarded = nil })
  return conversation(chat) :: agents_Conversation
end)

RPC.on_request("_act", function(params: agents_ActParams, ctx: vos_app_Context): agents_ActResult
  local chat = params.chat
  local agent, problem = ready(chat)
  if not agent then
    return ctx:fail(problem or "")
  end
  local row = timeline.get(chat, params.item)
  if not row or row.kind ~= "activity" then
    return ctx:fail("That surface is not in this chat.")
  end
  local action = codec.decode(params.action)
  if type(action) ~= "table" then
    return ctx:fail("The action is not a JSON object.")
  end
  touch(chat, nil)
  -- how A2UI over AG-UI carries what the person did on a surface: in the next run's forwarded props
  begin(chat, agent, { resume = nil, forwarded = { a2uiAction = { userAction = action } } })
  return conversation(chat) :: agents_Conversation
end)

RPC.on_request("_stop", function(params: agents_StopParams, ctx: vos_app_Context): agents_StopResult
  local mine = running[params.chat]
  if mine then
    mine.turn.stopped = true
    if mine.turn.stream then
      mine.turn.stream:close()
    end
    -- a cancelled task does not run again, so its own `finish` never happens: finish for it
    task.cancel(mine.task)
    finish(params.chat, mine)
  end
  return conversation(params.chat) or ctx:fail("There is no such chat.")
end)

RPC.on_request("_remove_chat", function(params: agents_RemoveChatParams): agents_RemoveChatResult
  local mine = running[params.chat]
  if mine then
    mine.turn.stopped = true
    if mine.turn.stream then
      mine.turn.stream:close()
    end
    task.cancel(mine.task)
    running[params.chat] = nil
  end
  db:tx(function()
    db:exec("delete from items where thread_id = :id", { id = params.chat })
    db:exec("delete from threads where id = :id", { id = params.chat })
    return nil
  end)
  return chats()
end)
