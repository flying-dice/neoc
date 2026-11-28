--!strict
-- The connections to the agents: one per chat in use, with the chat's ACP session on it. A connection is made the way
-- the agent's type says (transport.luau) when the chat takes a turn, kept for the next, and closed once the chat has
-- been idle for a while: `acpd` runs an agent process for each connection, a container agent's container runs while
-- its chat is connected, and a program agent's program runs with its connection, and none is kept running for
-- nothing.
--
-- The session is given MCP servers: the agent's own (their headers and variables read from the keychain now; an HTTP
-- one only reaches an agent that takes MCP servers over HTTP) and, for an agent that takes them over HTTP, this
-- machine's tools: the vos MCP server at the host's `/mcp`, with a bearer token the app leases for the agent
-- (`app.tools.lease`), at the address this machine has on the way to the agent. The lease goes with the connection.
local roster = require("./roster")
local task = require("std:task")
local timeline = require("./timeline")
local tracing = require("lib:tracing")
local transport = require("./transport")
local db = app.db

-- Seconds a chat's connection stays open after its last turn.
local IDLE = 300

-- A chat's connection: the session on it (once it is started), the agent it is to, how to close it, and the lease
-- its MCP server was given. `stale` when the agent was changed during a turn: the connection is closed when the turn
-- is over. `fresh` while its first turn runs.
export type Live = {
  cx: vnd_agent_client_protocol_ConnectionTo,
  close: () -> (),
  session: vnd_agent_client_protocol_ActiveSession?,
  agent: string,
  lease: string?,
  idle: std_task_Task?,
  stale: boolean,
  fresh: boolean,
}

local lives: { [string]: Live } = {}

local link = {}

-- the person's words for a failure of the socket to the agent or of ACP on it
link.sentence = transport.sentence

-- The chat's connection, when it has one.
function link.get(chat: string): Live?
  return lives[chat]
end

-- Close the chat's connection (and what goes with it: a container stops, a program ends), and end the lease its MCP
-- server was given.
function link.drop(chat: string)
  local live = lives[chat]
  if not live then
    return
  end
  lives[chat] = nil
  if live.idle then
    task.cancel(live.idle)
  end
  live.close()
  if live.lease then
    app.tools.release(live.lease)
  end
end

-- The agent was changed or removed: its connections go, now or (for a chat whose turn is running) when the turn is
-- over. The next turn connects with what the agent is now.
function link.forget(agent: string, running: (chat: string) -> boolean)
  for chat, live in lives do
    if live.agent == agent then
      if running(chat) then
        live.stale = true
      else
        link.drop(chat)
      end
    end
  end
end

-- The chat's turn is over: its connection stays for the next one, for a while.
function link.rest(chat: string)
  local live = lives[chat]
  if not live then
    return
  end
  if live.stale then
    link.drop(chat)
    return
  end
  live.fresh = false
  if live.idle then
    task.cancel(live.idle)
  end
  live.idle = task.delay(IDLE, function()
    if lives[chat] == live then
      link.drop(chat)
    end
  end)
end

-- The agent's own MCP servers as the session takes them, their headers and variables read from the keychain now; the
-- names of its HTTP ones, when the agent takes none over HTTP. Otherwise why they cannot be given.
local function serversOf(
  agent: roster.Agent,
  http: boolean
): ({ vnd_agent_client_protocol_McpServer }?, { string }, string?)
  local servers: { vnd_agent_client_protocol_McpServer } = {}
  local left: { string } = {}
  for _, server in agent.mcp do
    if server.type == "stdio" then
      local env, problem = roster.resolve(server.env or {})
      if not env then
        return nil, left, problem
      end
      table.insert(servers, {
        type = "stdio",
        name = server.name,
        command = server.command or "",
        args = server.args or {},
        env = env,
      })
    elseif http then
      local headers, problem = roster.resolve(server.headers or {})
      if not headers then
        return nil, left, problem
      end
      table.insert(servers, { type = "http", name = server.name, url = server.url or "", headers = headers })
    else
      table.insert(left, server.name)
    end
  end
  return servers, left, nil
end

-- The chat's connection, made when it has none, with its session: the chat's own loaded again when the agent can load
-- one, or a new one (and a note in the chat that the agent does not remember it). Otherwise the problem, in the
-- person's words, with what the agent wrote to its errors when it wrote something.
function link.attach(chat: string, agent: roster.Agent): (Live?, string?)
  local live = lives[chat]
  if live and not live.stale then
    if live.idle then
      task.cancel(live.idle)
      live.idle = nil
    end
    return live, nil
  end
  link.drop(chat)
  local opened, problem = transport.open(chat, agent)
  if not opened then
    return nil, problem
  end
  local cx = opened.cx
  local function quoted(said: string): string
    return transport.quoted(said, opened.errors())
  end
  -- kept at once, so that whatever ends it from here on (a stop, a failure) closes it and ends its lease
  local made: Live = { cx = cx, close = opened.close, agent = agent.id, stale = false, fresh = true }
  lives[chat] = made

  local capabilities = opened.agent.agent_capabilities
  local http = capabilities.mcp_capabilities.http
  local servers, left, unresolved = serversOf(agent, http)
  if not servers then
    link.drop(chat)
    return nil, unresolved
  end
  if http then
    local lease = app.tools.lease({ id = agent.id, name = agent.name }, { host = opened.host })
    made.lease = lease.token
    table.insert(servers, {
      type = "http",
      name = "vos",
      url = lease.url,
      headers = { { name = "Authorization", value = `Bearer {lease.token}` } },
    })
  end
  -- said once, when the session is new
  local unserved: string? = if #left > 0
    then `The agent does not take MCP servers over HTTP, so it was not given {table.concat(left, ", ")}.`
    else nil

  -- a session builder with the session's MCP servers
  local function with(builder: vnd_agent_client_protocol_SessionBuilder): vnd_agent_client_protocol_SessionBuilder
    for _, server in servers :: { vnd_agent_client_protocol_McpServer } do
      builder = builder:with_mcp_server(server)
    end
    return builder
  end

  local cwd = opened.cwd
  local row = db:one("select session from threads where id = :id", { id = chat })
  local before: string = if row then row.session else ""
  local afresh: string? = nil
  if before ~= "" and capabilities.load_session then
    local loaded, failed = with(cx:load_session(before, cwd)):start_session()
    if loaded then
      made.session = loaded
      return made, nil
    end
    -- the agent no longer has the session (it answers with an error): a new one; anything else ends here
    if not failed or failed.kind ~= "rpc" then
      link.drop(chat)
      return nil, quoted(link.sentence(failed))
    end
    tracing.warn("the agent could not load the chat's session", { error = failed.message, chat = chat })
    afresh =
      `The agent could not load this chat's session ({failed.message}), so it started a new one: it does not remember the earlier messages.`
  elseif before ~= "" then
    afresh =
      "The agent cannot load an earlier session, so it started a new one: it does not remember the earlier messages."
  end
  local started, failed = with(cx:build_session(cwd)):start_session()
  if not started then
    link.drop(chat)
    -- the agent's own refusal names the folder most often: one that is not there on its machine
    if failed and failed.kind == "rpc" then
      return nil, quoted(`The agent did not start a session in {cwd}: {failed.message}`)
    end
    return nil, quoted(link.sentence(failed))
  end
  made.session = started
  db:exec("update threads set session = :s where id = :id", { s = started.session_id, id = chat })
  if afresh then
    timeline.add(chat, "note", { text = afresh })
  end
  if unserved then
    timeline.add(chat, "note", { text = unserved })
  end
  return made, nil
end

return link
