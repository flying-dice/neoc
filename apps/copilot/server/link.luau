--!strict
-- The connections to the agents: one per chat in use, with the chat's ACP session on it. The agent runs as the chat's
-- container's program (sandbox.luau), and the connection is the container's standard streams, attached through Docker:
-- what the app writes to its input and what the agent writes to its output are ACP, a JSON-RPC message a line, and
-- what it writes to its errors is kept (the last few kilobytes) to say why it would not connect. A connection is made
-- when the chat takes a turn and kept for the next, and closed once the chat has been idle for a while; the container
-- is stopped with it, and the next turn starts it again and loads the chat's session.
--
-- An agent that can take MCP servers over HTTP is given this machine's tools in its session: the vos MCP server at the
-- host's `/mcp`, with a bearer token the app leases for the copilot (`app.tools.lease`), at the address this machine
-- has from inside a container (`host.docker.internal`). The lease goes with the connection. The copilot's own MCP
-- servers are given too: a stdio one runs in the container, an HTTP one only reaches an agent that takes them.
local acp = require("vnd:agent_client_protocol")
local copilots = require("./copilots")
local docker = require("vos:docker")
local sandbox = require("./sandbox")
local strings = require("lib:text")
local task = require("std:task")
local timeline = require("./timeline")
local db = app.db

-- Who this app says it is to an agent (`initialize`): the app and its version (package.json).
local CLIENT = { name = "vos", version = "0.1.0" }
-- Seconds the agent's answer to `initialize` may take: its program has just started.
local CONNECT_TIMEOUT = 60
-- Seconds a chat's connection stays open after its last turn.
local IDLE = 300
-- How much of what the agent wrote to its errors is kept, in bytes.
local ERRORS_KEPT = 4096
-- The longest message the agent may write, in bytes: one that never ends cannot fill the app's memory.
local MAX_LINE = 16 * 1024 * 1024
-- This machine, from inside a container (sandbox.luau maps it).
local HOST = "host.docker.internal"

-- A chat's connection: the session on it, the copilot it is to, the attached streams of its container, and the lease
-- its MCP server was given. `stale` when the copilot was changed during a turn: the connection is closed when the turn
-- is over. `fresh` while its first turn runs.
export type Live = {
  conn: vnd_agent_client_protocol_Connection,
  session: string,
  copilot: string,
  attach: vnd_bollard_Attach,
  lease: string?,
  idle: std_task_Task?,
  stale: boolean,
  fresh: boolean,
}

local lives: { [string]: Live } = {}

local link = {}

-- the person's words for a failure of `vnd:agent_client_protocol`
function link.sentence(err: vnd_agent_client_protocol_AcpError?): string
  if not err then
    return "The agent did not answer."
  end
  local kind, message = err.kind, err.message
  if kind == "connect" then
    return `The agent could not be reached: {message}`
  elseif kind == "status" then
    return `The agent refused the connection: {message}`
  elseif kind == "protocol" then
    return `The copilot's command did not answer as an ACP agent does: {message}`
  elseif kind == "rpc" then
    return `The agent answered with an error: {message}`
  elseif kind == "closed" then
    return `The connection to the agent closed: {message}`
  elseif kind == "timed_out" then
    return `The agent did not answer in time: {message}`
  end
  return `The agent was refused: {message}`
end

-- A problem, with what the agent wrote to its errors when it wrote something.
local function quoted(problem: string, errors: string): string
  local said = strings.trim(errors)
  return if said == "" then problem else `{problem}\n\nWhat the agent wrote to its errors:\n{said}`
end

-- The chat's connection, when it has one.
function link.get(chat: string): Live?
  return lives[chat]
end

-- Close the chat's connection and stop its container, and end the lease its MCP server was given.
function link.drop(chat: string)
  local live = lives[chat]
  if not live then
    return
  end
  lives[chat] = nil
  -- first, so that what is asked of the container next waits until it has stopped
  task.spawn(sandbox.stop, chat)
  if live.idle then
    task.cancel(live.idle)
  end
  live.conn:close()
  -- closing the connection leaves the container's input open: it is closed here
  live.attach:close_input()
  if live.lease then
    app.tools.release(live.lease)
  end
end

-- The copilot was changed or removed: its connections go, now or (for a chat whose turn is running) when the turn is
-- over. The next turn connects with what the copilot is now.
function link.forget(copilot: string, running: (chat: string) -> boolean)
  for chat, live in lives do
    if live.copilot == copilot then
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

-- The MCP servers of the copilot as the session takes them, their headers and variables read from the keychain now;
-- the names of its HTTP ones, when the agent takes none over HTTP. Otherwise why they cannot be given.
local function serversOf(
  copilot: copilots.Copilot,
  http: boolean
): ({ vnd_agent_client_protocol_McpServer }?, { string }, string?)
  local servers: { vnd_agent_client_protocol_McpServer } = {}
  local left: { string } = {}
  for _, server in copilot.mcp do
    if server.type == "stdio" then
      local env, problem = copilots.resolve(server.env or {})
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
      local headers, problem = copilots.resolve(server.headers or {})
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

-- The chat's connection, made when it has none: its container made or started, attached, and its session the chat's
-- own loaded again when the agent can load one, or a new one (and a note in the chat that the agent does not remember
-- it). Otherwise the problem, in the person's words.
function link.attach(chat: string, copilot: copilots.Copilot): (Live?, string?)
  local live = lives[chat]
  if live and not live.stale then
    if live.idle then
      task.cancel(live.idle)
      live.idle = nil
    end
    return live, nil
  end
  link.drop(chat)
  local workdir, problem = sandbox.ensure(chat, copilot)
  if not workdir then
    return nil, problem
  end
  local api, unreachable = docker.actor()
  if not api then
    return nil, unreachable
  end
  -- attached before it starts: nothing the agent writes is missed
  local attach, unattached =
    api:attach_container(sandbox.name(chat), { stream = true, stdin = true, stdout = true, stderr = true })
  if not attach then
    return nil, `The copilot's container could not be attached to: {docker.sentence(unattached)}`
  end
  local unstarted = sandbox.start(chat)
  if unstarted then
    attach:close_input()
    return nil, unstarted
  end

  -- what the agent wrote to its output and is not a whole line yet, where in it a line end is looked for next, and
  -- the end of what it wrote to its errors
  local pending, from, errors = "", 1, ""
  local function read(): string?
    while true do
      local cut = string.find(pending, "\n", from, true)
      if cut then
        local line = string.match(string.sub(pending, 1, cut - 1), "^(.-)\r?$") or ""
        pending, from = string.sub(pending, cut + 1), 1
        if line ~= "" then
          return line
        end
      else
        from = #pending + 1
        if #pending > MAX_LINE then
          error("The agent wrote a message longer than 16 MB.", 0)
        end
        -- the end of its output (the container stopped, or the connection closed) is the end of the agent's side
        local piece = attach:next()
        if not piece then
          return nil
        end
        if piece.stream == "stderr" then
          errors = string.sub(errors .. piece.data, -ERRORS_KEPT)
        else
          pending ..= piece.data
        end
      end
    end
  end
  local function write(line: string)
    local sent, failed = attach:write(line .. "\n")
    if not sent then
      error(`The copilot's container took no more input: {docker.sentence(failed)}`, 0)
    end
  end
  local conn, refused = acp.connect_lines({ read = read, write = write }, {
    timeout = CONNECT_TIMEOUT,
    client_info = CLIENT,
  })
  if not conn then
    task.spawn(sandbox.stop, chat)
    attach:close_input()
    return nil, quoted(link.sentence(refused), errors)
  end
  -- kept at once, so that whatever ends it from here on (a stop, a failure) closes it, stops the container and ends
  -- its lease
  local made: Live = { conn = conn, session = "", copilot = copilot.id, attach = attach, stale = false, fresh = true }
  lives[chat] = made

  local capabilities = conn.agent.agent_capabilities
  local http = capabilities.mcp_capabilities.http
  local servers, left, unresolved = serversOf(copilot, http)
  if not servers then
    link.drop(chat)
    return nil, unresolved
  end
  if http then
    local lease = app.tools.lease({ id = copilot.id, name = copilot.name }, { host = HOST })
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

  local row = db:one("select session from threads where id = :id", { id = chat })
  local before: string = if row then row.session else ""
  local afresh: string? = nil
  if before ~= "" and capabilities.load_session then
    local loaded, failed = conn:load_session { session_id = before, cwd = workdir, mcp_servers = servers }
    if loaded then
      made.session = before
      return made, nil
    end
    -- the agent no longer has the session (it answers with an error): a new one; anything else ends here
    if not failed or failed.kind ~= "rpc" then
      link.drop(chat)
      return nil, quoted(link.sentence(failed), errors)
    end
    afresh =
      `The agent could not load this chat's session ({failed.message}), so it started a new one: it does not remember the earlier messages.`
  elseif before ~= "" then
    afresh =
      "The agent cannot load an earlier session, so it started a new one: it does not remember the earlier messages."
  end
  local started, failed = conn:new_session { cwd = workdir, mcp_servers = servers }
  if not started then
    link.drop(chat)
    if failed and failed.kind == "rpc" then
      return nil, quoted(`The agent did not start a session in {workdir}: {failed.message}`, errors)
    end
    return nil, quoted(link.sentence(failed), errors)
  end
  made.session = started.session_id
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
