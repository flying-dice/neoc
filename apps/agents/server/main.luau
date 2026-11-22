--!strict
-- Agents: the list of chats with the agents on this host. The backend keeps only what the list needs (an id, the agent, a
-- title and when it was last used) in the app's database, and which chat was open (`app.kv`). The conversation itself
-- (messages, tool calls, permission prompts, the agent's settings) lives with the host as the AG-UI thread `agents:<id>`,
-- which the page reaches through the host page's `vos/agui/*` services: the backend never sees it.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local title = require("./title")
local db = app.db

-- the same step 1 as the view-tree Agents app it replaces, so chats made before are still listed
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
}

local LIST_MAX = 200
local OPEN_KEY = "open"

local function chatOf(row: { [string]: any }): rpc.Chat
  return {
    id = row.id,
    title = row.title,
    agentId = row.agent_id,
    agentName = row.agent_name,
    updatedAt = row.updated_at,
  }
end

local function find(id: string): rpc.Chat?
  local row = db:one("select id, title, agent_id, agent_name, updated_at from threads where id = :id", { id = id })
  return if row then chatOf(row) else nil
end

local function list(): rpc.ChatList
  local chats: { rpc.Chat } = {}
  for _, row in
    db:all(
      "select id, title, agent_id, agent_name, updated_at from threads order by updated_at desc, id desc limit :n",
      { n = LIST_MAX }
    )
  do
    table.insert(chats, chatOf(row))
  end
  local open = app.kv:get(OPEN_KEY)
  if type(open) ~= "string" or find(open) == nil then
    open = nil
  end
  return { chats = chats, open = open }
end

local handlers: rpc.Handlers = {
  agents = function(_ctx)
    local agents: { rpc.Agent } = {}
    for _, card in app.agents:list() do
      table.insert(agents, { id = card.id, name = card.name, role = card.role, avatar = card.avatar })
    end
    return agents
  end,

  chats = function(_ctx)
    return list()
  end,

  startChat = function(ctx, args)
    local agent = nil
    for _, card in app.agents:list() do
      if card.id == args.agentId then
        agent = card
      end
    end
    if agent == nil then
      return ctx:fail("There is no agent with that id on this host.")
    end
    local made = db:exec("insert into threads(agent_id, agent_name) values (:a, :n)", { a = agent.id, n = agent.name })
    local row = db:one(
      "select id, title, agent_id, agent_name, updated_at from threads where rowid = :r",
      { r = made.last_insert_id }
    )
    if row == nil then
      return ctx:fail("The chat could not be made.")
    end
    app.kv:set(OPEN_KEY, row.id)
    return chatOf(row)
  end,

  openChat = function(ctx, args)
    if args.id == nil then
      app.kv:delete(OPEN_KEY)
    elseif find(args.id) == nil then
      return ctx:fail("There is no chat with that id.")
    else
      app.kv:set(OPEN_KEY, args.id)
    end
    return list()
  end,

  noteMessage = function(ctx, args)
    if find(args.id) == nil then
      return ctx:fail("There is no chat with that id.")
    end
    db:exec(
      [[update threads
        set title = case when title = :new then :t else title end,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        where id = :id]],
      { new = title.NEW, t = title.of(args.text), id = args.id }
    )
    return find(args.id) :: rpc.Chat
  end,
}

return handlers
