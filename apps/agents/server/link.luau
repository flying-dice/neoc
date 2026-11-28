--!strict
-- The connections to the agents: one per chat in use, with the chat's ACP session on it. A connection is made when the
-- chat takes a turn and kept for the next, and closed once the chat has been idle for a while: `acpd` runs an agent
-- process for each connection, and an idle one is not kept running for nothing.
--
-- An agent that can take MCP servers over HTTP is given this machine's tools in its session: the vos MCP server at the
-- host's `/mcp`, with a bearer token the app leases for the agent (`app.tools.lease`), at the address this machine has
-- on the way to the agent. The lease goes with the connection.
local acp = require("vnd:agent_client_protocol")
local roster = require("./roster")
local task = require("std:task")
local timeline = require("./timeline")
local websocket = require("lib:websocket")
local db = app.db

-- Who this app says it is to an agent (`initialize`): the app and its version (package.json).
local CLIENT = { name = "vos", version = "0.6.1" }
-- Seconds the WebSocket to the agent may take to open.
local CONNECT_TIMEOUT = 30
-- Seconds a chat's connection stays open after its last turn.
local IDLE = 300

-- A chat's connection: the socket it runs on, the session on it, the agent it is to, and the lease its MCP server was
-- given. `stale` when the agent was changed during a turn: the connection is closed when the turn is over. `fresh`
-- while its first turn runs.
export type Live = {
  ws: lib_websocket_WebSocket,
  cx: vnd_agent_client_protocol_ConnectionTo,
  session: vnd_agent_client_protocol_ActiveSession?,
  agent: string,
  lease: string?,
  idle: std_task_Task?,
  stale: boolean,
  fresh: boolean,
}

local lives: { [string]: Live } = {}

local link = {}

-- the person's words for a failure of the socket to the agent (`lib:websocket`) or of ACP on it
-- (`vnd:agent_client_protocol`)
function link.sentence(err: (vnd_agent_client_protocol_AcpError | lib_websocket_WebSocketError)?): string
  if not err then
    return "The agent did not answer."
  end
  local kind, message = err.kind, err.message
  if kind == "connect" or kind == "tls" or kind == "connection_lost" then
    return `The agent could not be reached: {message}`
  elseif kind == "status" then
    return `The agent's address refused the connection: {message}`
  elseif kind == "protocol" then
    return `The address did not answer as an ACP agent does: {message}`
  elseif kind == "rpc" then
    return `The agent answered with an error: {message}`
  elseif kind == "closed" then
    return `The connection to the agent closed: {message}`
  elseif kind == "timed_out" then
    return `The agent did not answer in time: {message}`
  end
  return `The agent's address was refused: {message}`
end

-- The chat's connection, when it has one.
function link.get(chat: string): Live?
  return lives[chat]
end

-- Close the chat's connection, and end the lease its MCP server was given.
function link.drop(chat: string)
  local live = lives[chat]
  if not live then
    return
  end
  lives[chat] = nil
  if live.idle then
    task.cancel(live.idle)
  end
  live.cx:close()
  live.ws:close()
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

-- The chat's connection, made when it has none, with its session: the chat's own loaded again when the agent can load
-- one, or a new one (and a note in the chat that the agent does not remember it). Otherwise the problem, in the
-- person's words.
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
  local token, problem = roster.token(agent)
  if problem then
    return nil, problem
  end
  local ws, refused = websocket.connect(agent.url, {
    headers = if token then { Authorization = `Bearer {token}` } else nil,
    connect_timeout = CONNECT_TIMEOUT,
  })
  if not ws then
    return nil, link.sentence(refused)
  end
  local cx, broken = acp.Client.v1():name(CLIENT.name):connect((assert(acp.Lines.new(ws))))
  if not cx then
    ws:close()
    return nil, link.sentence(broken)
  end
  -- kept at once, so that whatever ends it from here on (a stop, a failure) closes it and ends its lease
  local made: Live = { ws = ws, cx = cx, agent = agent.id, stale = false, fresh = true }
  lives[chat] = made

  local initialized, unanswered = cx:send_request { protocol_version = 1, client_info = CLIENT }
  if not initialized then
    link.drop(chat)
    return nil, link.sentence(unanswered)
  end
  local capabilities = initialized.agent_capabilities
  local servers: { vnd_agent_client_protocol_McpServer } = {}
  local function with(builder: vnd_agent_client_protocol_SessionBuilder): vnd_agent_client_protocol_SessionBuilder
    for _, server in servers do
      builder = builder:with_mcp_server(server)
    end
    return builder
  end
  if capabilities.mcp_capabilities.http then
    local lease = app.tools.lease({ id = agent.id, name = agent.name }, { host = ws.local_address })
    made.lease = lease.token
    table.insert(servers, {
      type = "http",
      name = "vos",
      url = lease.url,
      headers = { { name = "Authorization", value = `Bearer {lease.token}` } },
    })
  end

  local row = db:one("select session from threads where id = :id", { id = chat })
  local before: string = if row then row.session else ""
  local afresh: string? = nil
  if before ~= "" and capabilities.load_session then
    local loaded, failed = with(cx:load_session(before, agent.folder)):start_session()
    if loaded then
      made.session = loaded
      return made, nil
    end
    -- the agent no longer has the session (it answers with an error): a new one; anything else ends here
    if not failed or failed.kind ~= "rpc" then
      link.drop(chat)
      return nil, link.sentence(failed)
    end
    afresh =
      `The agent could not load this chat's session ({failed.message}), so it started a new one: it does not remember the earlier messages.`
  elseif before ~= "" then
    afresh =
      "The agent cannot load an earlier session, so it started a new one: it does not remember the earlier messages."
  end
  local started, refusal = with(cx:build_session(agent.folder)):start_session()
  if not started then
    link.drop(chat)
    -- the agent's own refusal names the folder most often: one that is not there on its machine
    if refusal and refusal.kind == "rpc" then
      return nil, `The agent did not start a session in {agent.folder}: {refusal.message}`
    end
    return nil, link.sentence(refusal)
  end
  made.session = started
  db:exec("update threads set session = :s where id = :id", { s = started.session_id, id = chat })
  if afresh then
    timeline.add(chat, "note", { text = afresh })
  end
  return made, nil
end

return link
