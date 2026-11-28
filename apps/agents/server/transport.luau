--!strict
-- How a chat reaches its agent, by the agent's type: a connection that speaks the Agent Client Protocol (ACP), and
-- what goes with it. link.luau keeps it for the chat, and starts the chat's session on it.
--
-- - `remote`: a WebSocket to the agent's address (`acp.connect`), with its bearer token.
-- - `container`: the chat's container (sandbox.luau), made or started, its standard streams attached through Docker.
-- - `program`: the agent's command, started on this machine (`std:process`) in the agent's folder, its standard
--   streams piped.
--
-- A container's and a program's standard streams are ACP a JSON-RPC message a line (`acp.connect_lines`): what the
-- agent writes to its output is split into lines, and what it writes to its errors is kept (the last few kilobytes)
-- to say why it would not connect. Closing the connection stops the container, or closes the program's input and,
-- after a short while, kills it.
local acp = require("vnd:agent_client_protocol")
local docker = require("vos:docker")
local process = require("std:process")
local roster = require("./roster")
local sandbox = require("./sandbox")
local strings = require("lib:text")
local task = require("std:task")

-- Who this app says it is to an agent (`initialize`): the app and its version (package.json).
local CLIENT = { name = "vos", version = "0.7.0" }
-- Seconds a connection to a remote agent and its answer to `initialize` may take together.
local CONNECT_TIMEOUT = 30
-- Seconds the agent's answer to `initialize` may take when its program has just started.
local START_TIMEOUT = 60
-- How much of what the agent wrote to its errors is kept, in bytes.
local ERRORS_KEPT = 4096
-- The longest message the agent may write, in bytes: one that never ends cannot fill the app's memory.
local MAX_LINE = 16 * 1024 * 1024
-- The most a program's input takes in one write (`std:process`).
local WRITE_MAX = 1024 * 1024
-- Seconds a program has to end once its input is closed, before it is killed.
local GRACE = 3
-- This machine, from inside a container (sandbox.luau maps it).
local DOCKER_HOST = "host.docker.internal"

-- A chat's connection to its agent. `close` ends it and what goes with it (the container stops, the program ends).
-- `host` is this machine's address as the agent reaches it, for the vos MCP server (nil: the host's own default).
-- `cwd` is the folder its sessions work in. `errors` is the end of what the agent wrote to its errors.
export type Opened = {
  conn: vnd_agent_client_protocol_Connection,
  close: () -> (),
  host: string?,
  cwd: string,
  errors: () -> string,
}

local transport = {}

-- the person's words for a failure of `vnd:agent_client_protocol`
function transport.sentence(err: vnd_agent_client_protocol_AcpError?): string
  if not err then
    return "The agent did not answer."
  end
  local kind, message = err.kind, err.message
  if kind == "connect" then
    return `The agent could not be reached: {message}`
  elseif kind == "status" then
    return `The agent's address refused the connection: {message}`
  elseif kind == "protocol" then
    return `The agent did not answer as an ACP agent does: {message}`
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
function transport.quoted(problem: string, errors: string): string
  local said = strings.trim(errors)
  return if said == "" then problem else `{problem}\n\nWhat the agent wrote to its errors:\n{said}`
end

-- The end of what an agent wrote to its errors: the last few kilobytes.
type Ring = { add: (data: string) -> (), text: () -> string }
local function ring(): Ring
  local kept = ""
  return {
    add = function(data: string)
      kept = string.sub(kept .. data, -ERRORS_KEPT)
    end,
    text = function(): string
      return kept
    end,
  }
end

-- The agent's messages, a line at a time, from what it writes to its output: `pull` gives the next piece of it, or
-- nil at its end, which is the end of the agent's side. A blank line is no message.
local function lines(pull: () -> string?): () -> string?
  -- what the agent wrote and is not a whole line yet, and where in it a line end is looked for next
  local pending, from = "", 1
  return function(): string?
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
        local piece = pull()
        if not piece then
          return nil
        end
        pending ..= piece
      end
    end
  end
end

local function nothing(): string
  return ""
end

-- A remote agent: a WebSocket to its address.
local function remote(_chat: string, agent: roster.Agent): (Opened?, string?)
  local token, problem = roster.token(agent)
  if problem then
    return nil, problem
  end
  local conn, refused = acp.connect(agent.url, {
    headers = if token then { Authorization = `Bearer {token}` } else nil,
    timeout = CONNECT_TIMEOUT,
    client_info = CLIENT,
  })
  if not conn then
    return nil, transport.sentence(refused)
  end
  local opened = conn
  return {
    conn = opened,
    close = function()
      opened:close()
    end,
    host = opened.local_address,
    cwd = agent.folder,
    errors = nothing,
  },
    nil
end

-- A container agent: the chat's container, attached to before it starts, so nothing the agent writes is missed.
local function container(chat: string, agent: roster.Agent): (Opened?, string?)
  local workdir, problem = sandbox.ensure(chat, agent)
  if not workdir then
    return nil, problem
  end
  local api, unreachable = docker.actor()
  if not api then
    return nil, unreachable
  end
  local attach, unattached =
    api:attach_container(sandbox.name(chat), { stream = true, stdin = true, stdout = true, stderr = true })
  if not attach then
    return nil, `The agent's container could not be attached to: {docker.sentence(unattached)}`
  end
  local unstarted = sandbox.start(chat)
  if unstarted then
    attach:close_input()
    return nil, unstarted
  end
  local errors = ring()
  local read = lines(function(): string?
    while true do
      -- the end of its output (the container stopped, or the connection closed) is the end of the agent's side
      local piece = attach:next()
      if not piece then
        return nil
      end
      if piece.stream == "stderr" then
        errors.add(piece.data)
      else
        return piece.data
      end
    end
  end)
  local function write(line: string)
    local sent, failed = attach:write(line .. "\n")
    if not sent then
      error(`The agent's container took no more input: {docker.sentence(failed)}`, 0)
    end
  end
  -- first, so that what is asked of the container next waits until it has stopped; closing the connection leaves
  -- the container's input open, so it is closed here
  local function close()
    task.spawn(sandbox.stop, chat)
    attach:close_input()
  end
  local conn, refused = acp.connect_lines({ read = read, write = write }, {
    timeout = START_TIMEOUT,
    client_info = CLIENT,
  })
  if not conn then
    close()
    return nil, transport.quoted(transport.sentence(refused), errors.text())
  end
  local opened = conn
  return {
    conn = opened,
    close = function()
      opened:close()
      close()
    end,
    host = DOCKER_HOST,
    cwd = workdir,
    errors = errors.text,
  },
    nil
end

-- the person's words for a program that would not start
local function unstarted(agent: roster.Agent, err: std_process_ProcessError?): string
  local program = agent.command[1] or ""
  if err and err.kind == "spawn" then
    if err.cause == "not_found" then
      return `There is no program {program} on this machine: not at that path, and not in PATH.`
    elseif err.cause == "permission_denied" then
      return `The program {program} may not be run.`
    elseif err.cause == "no_directory" then
      return `There is no folder {agent.folder} on this machine for the program to work in.`
    end
  end
  return `The program {program} did not start: {if err then err.message else "it did not say why"}`
end

-- A program agent: its command, run on this machine in its folder, with its environment (a keychain password read
-- now, and never kept).
local function program(_chat: string, agent: roster.Agent): (Opened?, string?)
  local resolved, unresolved = roster.resolve(agent.env)
  if not resolved then
    return nil, unresolved
  end
  local env: { [string]: string } = {}
  for _, pair in resolved do
    env[pair.name] = pair.value
  end
  local args = table.clone(agent.command)
  local name = table.remove(args, 1) or ""
  local child, refused = process.spawn {
    program = name,
    args = args,
    cwd = agent.folder,
    env = env,
    stdin = "piped",
    stdout = "piped",
    stderr = "piped",
  }
  if not child then
    return nil, unstarted(agent, refused)
  end
  local running = child
  local errors = ring()
  -- what it writes to its errors is read as it comes, so that the pipe never fills
  task.spawn(function()
    while true do
      local bytes = running:read_stderr()
      if not bytes then
        return
      end
      errors.add(bytes)
    end
  end)
  local read = lines(function(): string?
    local bytes, err = running:read()
    if err then
      error(`The agent's output could not be read: {err.message}`, 0)
    end
    return bytes
  end)
  local function write(line: string)
    local message = line .. "\n"
    for at = 1, #message, WRITE_MAX do
      local sent, failed = running:write(string.sub(message, at, at + WRITE_MAX - 1))
      if not sent then
        error(`The agent took no more input: {if failed then failed.message else "it did not say why"}`, 0)
      end
    end
  end
  -- the program is asked to end (its input ends), and is killed when it has not after a while
  local function close()
    running:close_stdin()
    task.delay(GRACE, function()
      running:kill()
    end)
  end
  local conn, failed = acp.connect_lines({ read = read, write = write }, {
    timeout = START_TIMEOUT,
    client_info = CLIENT,
  })
  if not conn then
    close()
    return nil, transport.quoted(transport.sentence(failed), errors.text())
  end
  local opened = conn
  return {
    conn = opened,
    close = function()
      opened:close()
      close()
    end,
    host = nil,
    cwd = agent.folder,
    errors = errors.text,
  },
    nil
end

-- The chat's connection to its agent, made the way its type says. Otherwise the problem, in the person's words.
function transport.open(chat: string, agent: roster.Agent): (Opened?, string?)
  if agent.kind == "container" then
    return container(chat, agent)
  elseif agent.kind == "program" then
    return program(chat, agent)
  end
  return remote(chat, agent)
end

return transport
