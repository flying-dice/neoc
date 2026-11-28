--!strict
-- The Agents app's backend: the agents the person added (roster.luau), the chats with them, and the agent's turn in a
-- chat (turn.luau). An agent speaks the Agent Client Protocol (ACP): each chat is an ACP session of the agent's, on a
-- connection the backend keeps while the chat is in use (link.luau). How the connection is made is the agent's type
-- (transport.luau): a WebSocket to a remote agent's address, the standard streams of a container of the chat's own
-- (sandbox.luau, on the container framework `vos:docker`, from an image the Containers app built or pulled), or those
-- of a program run on this machine. Everything is the app's own: the agents, chats and items are in its database, and a
-- token is in the keychain. A container agent's skills are the Skills app's (skills.luau), and a keychain item a
-- variable takes its value from is listed by the Keychain app.
--
-- It serves on the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`. The page's methods are
-- private (`_agents`, `_send`, ...); the app has no tools of its own, and gives an agent that takes them the tools of
-- the apps that are running, over the host's MCP endpoint (link.luau).
--
-- A turn: `_send` stores what the person said, answers at once, and leaves a task running the agent's turn. What the
-- agent says and does reaches the open window as `item` notifications (a whole item, new or changed) and `piece`
-- notifications (text added to an item); a `turn` notification says the turn is over (complete, stopped or failed). A
-- window that missed some reads the chat again with `_open`. A permission the agent asks for during its turn is
-- answered with `_answer`, and the turn goes on.
local codec = require("./codec")
local docker = require("vos:docker")
local images = require("./images")
local link = require("./link")
local roster = require("./roster")
local sandbox = require("./sandbox")
local skills = require("./skills")
local strings = require("lib:text")
local task = require("std:task")
local tracing = require("lib:tracing")
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
  -- ACP agents replace the AG-UI ones: an AG-UI chat cannot go on with an ACP agent, so the agents go and their chats
  -- with them. An agent has the folder its sessions work in, and a chat the ACP session it is. The tokens of the agents
  -- that go are listed to be forgotten: the keychain is not in the database.
  [4] = [[
    create table forgotten_tokens as select id from agents where has_token = 1;
    delete from items;
    delete from threads;
    delete from agents;
    alter table agents add column folder text not null default '/';
    alter table threads drop column state;
    alter table threads add column session text not null default '';
  ]],
  -- an agent's picture: a `data:` URL, or empty for none
  [5] = [[
    alter table agents add column avatar text not null default '';
  ]],
  -- a chat is made by the first thing said in it: the ones opened and left empty go
  [6] = [[
    delete from threads where not exists (select 1 from items where items.thread_id = threads.id);
  ]],
  -- an agent has a type: the agents there are remote ones, and keep their chats. A container agent has an image, a
  -- command, a skills folder and the names of the skills it uses; a program agent a command; both an environment.
  -- Every agent may have MCP servers. A chat with a container agent may have a container (and a volume) in Docker,
  -- and the folder its sessions start in, there.
  [7] = [[
    alter table agents add column kind text not null default 'remote';
    alter table agents add column command text not null default '[]';
    alter table agents add column image text not null default '';
    alter table agents add column skills_dir text not null default '';
    alter table agents add column skills text not null default '[]';
    alter table agents add column env text not null default '[]';
    alter table agents add column mcp text not null default '[]';
    alter table threads add column sandbox integer not null default 0;
    alter table threads add column workdir text not null default '';
    create index threads_by_agent on threads(agent_id);
  ]],
}

-- the tokens of the agents migration 4 removed
if db:one("select 1 as found from sqlite_master where type = 'table' and name = 'forgotten_tokens'") then
  for _, row in db:all("select id from forgotten_tokens") do
    roster.forget(row.id)
  end
  db:exec("drop table forgotten_tokens")
end

-- no turn outlives the backend: what was still coming when it last stopped is as long as it will get, and a question
-- the agent asked in it has no turn to go back to
db:exec("update items set done = 1 where done = 0")

-- the turn in progress in each chat, and the task running it
type Running = { task: any, turn: turn.Turn }
local running: { [string]: Running } = {}

-- Seconds an agent has to end its turn after it is asked to stop; one that does not is disconnected.
local STOP_GRACE = 10

local TITLE_MAX = 48

-- the first message names the chat; later messages only move it to the top
local function titleOf(text: string): string
  local line = strings.trim(string.match(strings.trim_start(text), "^[^\n]*") or "")
  return if line == "" then "New chat" else strings.clip(line, TITLE_MAX, "…")
end

local function chatOf(row: { [string]: any }): agents_Chat
  return { id = row.id, title = row.title, agent = row.agent_name, agent_id = row.agent_id, updated = row.updated_at }
end

local function chats(): { agents_Chat }
  local list: { agents_Chat } = {}
  for _, row in
    db:all(
      "select id, title, agent_id, agent_name, updated_at from threads order by updated_at desc, id desc limit 200"
    )
  do
    table.insert(list, chatOf(row))
  end
  return list
end

-- whether the agent asked something in the chat that has no answer yet
local function asking(chat: string): boolean
  return db:one(
    "select 1 as found from items where thread_id = :c and kind = 'permission' and done = 0 limit 1",
    { c = chat }
  ) ~= nil
end

local function conversation(id: string): agents_Conversation?
  local row = db:one("select id, title, agent_id, agent_name, updated_at from threads where id = :id", { id = id })
  if not row then
    return nil
  end
  return { chat = chatOf(row), items = timeline.items(id), busy = running[id] ~= nil, waiting = asking(id) }
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

local function busy(chat: string): boolean
  return running[chat] ~= nil
end

-- the turn in `chat` is over, however it ended
local function finish(chat: string, mine: Running)
  timeline.settle(chat)
  if running[chat] == mine then
    running[chat] = nil
    link.rest(chat)
  end
  RPC.send_notification("_turn", { chat = chat })
end

-- End the turn in `chat` here and now: its task is cancelled, and its connection closed.
local function halt(chat: string, mine: Running)
  mine.turn.stopped = true
  -- a cancelled task does not run again, so its own `finish` never happens: finish for it
  task.cancel(mine.task)
  link.drop(chat)
  finish(chat, mine)
end

-- The turn: on the chat's connection, made (or made again) as needed. A connection kept from an earlier turn may have
-- closed since (the agent's process ended): then it is made again, and the prompt sent once more.
local function converse(chat: string, agent: roster.Agent, text: string, current: turn.Turn)
  for _ = 1, 2 do
    local live, problem = link.attach(chat, agent)
    if not live then
      tracing.warn("could not reach the agent", { error = problem, chat = chat, agent = agent.id })
      timeline.add(chat, "error", { text = problem or "The agent could not be reached." })
      return
    end
    if current.stopped then
      return
    end
    if turn.take(chat, live, text, current, not live.fresh) ~= "lost" then
      return
    end
    link.drop(chat)
  end
end

-- Leave the agent's turn running in `chat`. Deferred: the caller has answered before the agent says anything.
local function begin(chat: string, agent: roster.Agent, text: string)
  local mine: Running = { task = nil, turn = { session = nil, stopped = false } }
  running[chat] = mine
  mine.task = task.defer(function()
    local ok, problem = pcall(function(): string?
      converse(chat, agent, text, mine.turn)
      return nil
    end)
    if not ok then
      tracing.error("the agent's turn stopped", { error = tostring(problem), chat = chat })
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

-- Delete a chat and its items from the database; a container chat's container and volume are removed first.
local function forget(chat: string)
  db:tx(function()
    db:exec("delete from items where thread_id = :id", { id = chat })
    db:exec("delete from threads where id = :id", { id = chat })
    return nil
  end)
end

-- What the page gives of an agent, as roster.luau takes it.
local function fieldsOf(params: agents_AddAgentParams | agents_UpdateAgentParams): roster.Fields
  return {
    name = params.name,
    url = params.url,
    folder = params.folder,
    command = params.command,
    image = params.image,
    skills_dir = params.skills_dir,
    skills = params.skills,
    env = params.env,
    mcp = params.mcp,
  }
end

RPC.on_request("_agents", function(): agents_AgentsResult
  return roster.all()
end)

RPC.on_request("_add_agent", function(params: agents_AddAgentParams, ctx: vos_app_Context): agents_AddAgentResult
  local kind: roster.Kind = params.kind or "remote"
  if kind ~= "remote" and kind ~= "container" and kind ~= "program" then
    return ctx:fail("An agent is remote, a container or a program.")
  end
  local problem = roster.add(kind, fieldsOf(params), params.token, params.avatar)
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
    local problem = roster.update(
      agent,
      fieldsOf(params),
      params.token,
      params.forget == true,
      params.avatar,
      params.remove_avatar == true
    )
    if problem then
      return ctx:fail(problem)
    end
    -- the next turn connects to the agent as it is now
    link.forget(agent.id, busy)
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
    local threads = db:all("select id, sandbox from threads where agent_id = :a", { a = agent.id })
    -- its turns end with it
    for _, row in threads do
      local mine = running[row.id]
      if mine then
        halt(row.id, mine)
      end
    end
    link.forget(agent.id, busy)
    -- a container agent's chats go with their containers and volumes: one Docker would not remove stays, with the
    -- agent. Its image is the Containers app's, and stays.
    if agent.kind == "container" then
      for _, row in threads do
        link.drop(row.id)
        if row.sandbox == 1 then
          local problem = sandbox.remove(row.id)
          if problem then
            return ctx:fail(problem)
          end
        end
        forget(row.id)
      end
    end
    roster.remove(agent)
    return roster.all()
  end
)

RPC.on_request("_engine", function(): agents_EngineResult
  return docker.status()
end)

RPC.on_request("_connect", function(params: agents_ConnectParams, ctx: vos_app_Context): agents_ConnectResult
  local refused = docker.use(params.host)
  return if refused then ctx:fail(refused) else docker.status()
end)

-- The Keychain app's items, for a picker: it keeps them, and says which there are without their passwords.
RPC.on_request("_secrets", function(_params: {}, ctx: vos_app_Context): agents_SecretsResult
  local listed: keychain_ListKeychainItemsResult?, err = RPC.send_request("keychain", "list_keychain_items")
  if not listed then
    if err and err.kind == "not_found" then
      return ctx:fail("The Keychain app is not there: install it, or open it, to choose one of its items.")
    end
    return ctx:fail(`The Keychain app did not list its items: {if err then err.message else "it did not say why"}`)
  end
  local secrets: { agents_Secret } = {}
  for _, item in listed.items do
    table.insert(secrets, { service = item.service, account = item.account })
  end
  return secrets
end)

RPC.on_request("_skills", function(): agents_SkillsResult
  return skills.list()
end)

RPC.on_request("_images", function(): agents_ImagesResult
  return images.list()
end)

RPC.on_request("_chats", function(): agents_ChatsResult
  return chats()
end)

RPC.on_request("_open", function(params: agents_OpenParams, ctx: vos_app_Context): agents_OpenResult
  return conversation(params.chat) or ctx:fail("There is no such chat.")
end)

-- A chat is made by the first thing said in it: a new one is `agent` instead of `chat`, so opening a chat and leaving
-- it unsaid leaves nothing behind.
RPC.on_request("_send", function(params: agents_SendParams, ctx: vos_app_Context): agents_SendResult
  local text = strings.trim(params.text)
  if text == "" then
    return ctx:fail("Write something first.")
  end
  local chat: string
  if params.chat then
    chat = params.chat
  else
    local with = if params.agent then roster.find(params.agent) else nil
    if not with then
      return ctx:fail("There is no such agent.")
    end
    local made = db:exec("insert into threads(agent_id, agent_name) values (:a, :n)", { a = with.id, n = with.name })
    chat = (db:one("select id from threads where rowid = :r", { r = made.last_insert_id }) :: { [string]: any }).id
  end
  local agent, problem = ready(chat)
  if not agent then
    return ctx:fail(problem or "")
  end
  timeline.add(chat, "user", { text = text })
  touch(chat, titleOf(text))
  begin(chat, agent, text)
  return conversation(chat) :: agents_Conversation
end)

RPC.on_request("_answer", function(params: agents_AnswerParams, ctx: vos_app_Context): agents_AnswerResult
  local chat = params.chat
  local mine = running[chat]
  local row = timeline.get(chat, params.item)
  local session = if mine then mine.turn.session else nil
  if not row or row.kind ~= "permission" or row.done or not session then
    return ctx:fail("That question is not open any more.")
  end
  local data = codec.decode(row.data) or {}
  local offered = false
  for _, option in data.options or {} do
    offered = offered or option.option_id == params.option
  end
  if not offered then
    return ctx:fail("The agent did not offer that answer.")
  end
  local answered = session:respond_permission(row.key, { outcome = "selected", option_id = params.option })
  if not answered then
    return ctx:fail("That question is not open any more.")
  end
  data.chosen = params.option
  timeline.set(chat, row.id, { data = codec.encode(data), done = true })
  return conversation(chat) :: agents_Conversation
end)

RPC.on_request("_stop", function(params: agents_StopParams, ctx: vos_app_Context): agents_StopResult
  local chat = params.chat
  local mine = running[chat]
  if mine then
    mine.turn.stopped = true
    local live = link.get(chat)
    -- the agent is asked to stop (its open questions are answered `cancelled`), and ends its turn when it has
    local session = mine.turn.session
    if session and live and live.cx:send_notification { session_id = session.session_id } then
      task.delay(STOP_GRACE, function()
        if running[chat] == mine then
          link.drop(chat)
        end
      end)
    else
      -- nothing to ask yet (it is still connecting), or the asking failed: the turn ends here
      halt(chat, mine)
    end
  end
  return conversation(chat) or ctx:fail("There is no such chat.")
end)

RPC.on_request("_remove_chat", function(params: agents_RemoveChatParams, ctx: vos_app_Context): agents_RemoveChatResult
  local chat = params.chat
  local mine = running[chat]
  if mine then
    mine.turn.stopped = true
    task.cancel(mine.task)
    running[chat] = nil
  end
  -- a program chat's program ends with its connection, a container chat's container stops
  link.drop(chat)
  local row = db:one("select sandbox from threads where id = :id", { id = chat })
  if row and row.sandbox == 1 then
    local problem = sandbox.remove(chat)
    if problem then
      return ctx:fail(problem)
    end
  end
  forget(chat)
  return chats()
end)
