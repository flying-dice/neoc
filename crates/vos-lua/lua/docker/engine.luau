--!strict
-- Docker, as an app reaches it: which one (the person's choice, kept in the app's settings; this machine's own with
-- none chosen), the clients that talk to it (through `vnd:bollard`), why a call failed, as a sentence, and the watch
-- that tells the app when what Docker has may be different. What an app asks of Docker is the app's own, or
-- `owned.luau` for an app with one container of its own.
local app = require("vos:app")
local bollard = require("vnd:bollard")
local task = require("std:task")
local text = require("./text")

local engine = {}

-- the setting: where Docker listens; absent for this machine's own
local HOST = "host"
-- How long a call may wait for Docker's answer, in seconds. Reading is quick or Docker is not answering (a Docker
-- that is starting, or paused, takes the connection and says nothing): the window should hear of that soon. Acting
-- takes as long as the container does: Docker gives one ten seconds to stop before it kills it. Pulling an image
-- takes as long as the network does.
local READ_TIMEOUT = 5
local ACT_TIMEOUT = 90
local PULL_TIMEOUT = 600
-- how often a Docker that is away is looked for again, in seconds
local RETRY = 5
-- the most of a container's output that is read and sent, in bytes
local LOG_LIMIT = 256 * 1024
local DEFAULT_TAIL = 200
local MAX_TAIL = 10000
-- the longest address that is kept, in bytes
local HOST_LIMIT = 300

export type Client = vnd_bollard_Client
type Clients = { reading: Client, acting: Client, pulling: Client }
type Events = mlua_Stream<vnd_bollard_EventMessage, vnd_bollard_DockerError>

-- Whether Docker can be reached, as a page shows it: its version when it can, why not (`problem`) when it cannot.
export type Status = vos_docker_Status
-- The end of what a container wrote. `truncated`: there was more before it.
export type Logs = vos_docker_Logs

-- the clients of the chosen Docker, made on first use and again after the choice changes
local clients: Clients? = nil
-- the events the watchers are reading, so that a change of Docker can end them
local watched: { [Events]: true } = {}
-- counts the choices of a Docker: a watcher that sees it move looks for the new one at once
local chosen = 0

function engine.host(): string
  local host = app.kv:get(HOST)
  return if type(host) == "string" then host else ""
end

-- Why a call failed, as a sentence for the person. Docker's own message is one already ("port is already
-- allocated"); no Docker at all is said in the framework's words.
function engine.sentence(err: vnd_bollard_DockerError?): string
  if not err then
    return "Docker refused, and did not say why."
  end
  if err.kind == "connect" then
    local host = engine.host()
    return if host == ""
      then "Docker is not running on this machine, or cannot be reached."
      else `Docker cannot be reached at {host}.`
  elseif err.kind == "timed_out" then
    return "Docker is not answering. It may still be starting, or be paused."
  end
  return if err.message ~= "" then err.message else "Docker refused, and did not say why."
end

local function connected(): (Clients?, string?)
  if clients then
    return clients, nil
  end
  local host = if engine.host() == "" then nil else engine.host()
  local made: { Client } = {}
  for _, timeout in { READ_TIMEOUT, ACT_TIMEOUT, PULL_TIMEOUT } do
    local client, err = bollard.connect { host = host, timeout = timeout }
    if not client then
      return nil, engine.sentence(err)
    end
    table.insert(made, client)
  end
  clients = { reading = made[1], acting = made[2], pulling = made[3] }
  return clients, nil
end

-- the client that reads what Docker has
function engine.reader(): (Client?, string?)
  local all, problem = connected()
  return if all then all.reading else nil, problem
end

-- the client that changes it
function engine.actor(): (Client?, string?)
  local all, problem = connected()
  return if all then all.acting else nil, problem
end

-- the client that pulls an image
function engine.puller(): (Client?, string?)
  local all, problem = connected()
  return if all then all.pulling else nil, problem
end

-- Whether Docker can be reached. Never fails: a Docker that is away is an answer.
function engine.status(): Status
  local status: Status = { reachable = false, version = "", host = engine.host(), problem = "" }
  local api, problem = engine.reader()
  if not api then
    status.problem = problem or ""
    return status
  end
  local version, err = api:version()
  if not version then
    status.problem = engine.sentence(err)
    return status
  end
  status.reachable = true
  status.version = version.Version or ""
  return status
end

-- Choose the Docker the app uses: an address, or empty for this machine's own. The address is only checked for
-- being one (a Docker that is not running there is said by `engine.status`, and may start later). Answers with why
-- the address was refused, and with nothing when it was taken.
function engine.use(host: string): string?
  if type(host) ~= "string" or #host > HOST_LIMIT then
    return "That is not an address of a Docker."
  end
  host = string.match(host, "^%s*(.-)%s*$") :: string
  if host ~= "" then
    if
      not (string.match(host, "^unix:///") or string.match(host, "^tcp://[^/]") or string.match(host, "^https?://[^/]"))
    then
      return "The address must start with unix:///, tcp:// or http://, or be empty for this machine's own Docker."
    end
    -- a socket that is not there is a fine choice (Docker may not be running yet): only an address that is none is not
    local _, err = bollard.connect { host = host }
    if err and err.kind ~= "connect" then
      return engine.sentence(err)
    end
  end
  if host == "" then
    app.kv:delete(HOST)
  else
    app.kv:set(HOST, host)
  end
  clients = nil
  -- the watchers read the events of the Docker that was chosen before: end them, and they look for the new one
  chosen += 1
  for events in watched do
    events:close()
  end
  return nil
end

-- The end of what a container wrote, by id or name: its output and its errors together in the order Docker has them,
-- as text fit to show (valid UTF-8, without a terminal's colour sequences and control characters). `tail` is how many
-- lines from the end.
function engine.logs(container: string, tail: number?): (Logs?, string?)
  local api, problem = engine.reader()
  if not api then
    return nil, problem
  end
  -- a number that is none (NaN) is no number of lines
  local asked = if type(tail) == "number" and tail == tail then tail else DEFAULT_TAIL
  local lines = math.clamp(math.floor(asked), 1, MAX_TAIL)
  -- the request is made as the stream is read: a container that is not there is the error of its first piece
  local stream = api:logs(container, { tail = lines })
  local pieces: { string } = {}
  local read = 0
  while true do
    local piece, piece_err = stream:next()
    if piece_err then
      return nil, engine.sentence(piece_err)
    end
    if not piece then
      break
    end
    table.insert(pieces, piece.data)
    read += #piece.data
    -- lines without end are not kept whole: what is shown is the end, and the first pieces are not part of it
    while read - #pieces[1] > LOG_LIMIT do
      read -= #(table.remove(pieces, 1) :: string)
    end
  end
  local written, cut = text.tail(table.concat(pieces), LOG_LIMIT)
  local _, breaks = string.gsub(written, "\n", "")
  return { text = written, truncated = cut or breaks >= lines }, nil
end

-- Watch Docker for as long as the app is loaded, and call `changed` whenever what it has of `filters` (Docker's own,
-- for its events: `{ type = { "container" }, event = { "start", "die" } }`) may be different: a container was made,
-- started, stopped or removed, Docker itself came or went, another Docker was chosen, or the watcher was not listening
-- for a while (Docker does not repeat what it reported to nobody). A Docker that is away is looked for again without
-- the app's doing. It never returns: run it in a task (`task.spawn(engine.watch, filters, changed)`).
function engine.watch(filters: { [string]: { string } }, changed: () -> ())
  local reachable: boolean? = nil
  while true do
    local choice = chosen
    local api = engine.reader()
    local events = if api and api:ping() then api:events { filters = filters } else nil
    if events then
      reachable = true
      watched[events] = true
      -- another Docker was chosen while this one was asked: `engine.use` did not know of these events to close them
      if choice ~= chosen then
        events:close()
      end
      -- from here on every change is heard; what happened before this was not
      changed()
      -- ends when Docker goes away, and when another Docker is chosen (`engine.use` closes it)
      while events:next() do
        changed()
      end
      watched[events] = nil
      events:close()
      -- whatever ended them, the windows read again: a Docker that went away is said at once
      changed()
    elseif reachable ~= false then
      reachable = false
      changed()
    end
    -- a Docker that is away is looked for again in a while; one that was just chosen, now
    if choice == chosen then
      task.wait(RETRY)
    end
  end
end

return engine
