--!strict
-- Docker, as this app reaches it: which one (the person's choice, kept in the app's settings; this machine's own with
-- none chosen), the clients that talk to it (through `vnd:bollard`), and why a call failed, as a sentence. What is
-- asked of Docker is in sidecar.luau.
local app = require("vos:app")
local bollard = require("vnd:bollard")
local task = require("std:task")
local trim = require("lib:text").trim

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

export type Client = vnd_bollard_Client
type Clients = { reading: Client, acting: Client, pulling: Client }

-- the clients of the chosen Docker, made on first use and again after the choice changes
local clients: Clients? = nil
-- the events the watcher is reading, so that a change of Docker can end them
local watched: mlua_Stream<vnd_bollard_EventMessage, vnd_bollard_DockerError>? = nil
-- another Docker was chosen since the watcher last looked: it looks for the new one at once
local switched = false

function engine.host(): string
  local host = app.kv:get(HOST)
  return if type(host) == "string" then host else ""
end

-- Why a call failed, as a sentence for the person. Docker's own message is one already ("port is already
-- allocated"); no Docker at all is said in the app's words.
function engine.sentence(err: vnd_bollard_DockerError): string
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
    if err then
      return nil, engine.sentence(err)
    end
    table.insert(made, client :: Client)
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

-- Docker's version, or why it cannot be reached
function engine.version(): (string?, string?)
  local api, problem = engine.reader()
  if not api then
    return nil, problem
  end
  local version, err = api:version()
  if err then
    return nil, engine.sentence(err)
  end
  return (version and version.Version) or "", nil
end

-- Choose the Docker the server runs in: an address, or empty for this machine's own. The address is only checked
-- for being one (a Docker that is not running there is said by the overview, and may start later).
function engine.use(host: string): string?
  host = trim(host)
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
  -- the watcher reads the events of the Docker that was chosen before: end them, and it looks for the new one
  switched = true
  if watched then
    watched:close()
  end
  return nil
end

-- Watch Docker for as long as the app is loaded, and call `changed` whenever what it has of `filters` may be
-- different: a container was made, started, stopped or removed, Docker itself came or went, or the watcher was not
-- listening for a while (Docker does not repeat what it reported to nobody). It never returns: run it in a task.
function engine.watch(filters: { [string]: { string } }, changed: () -> ())
  local reachable: boolean? = nil
  while true do
    switched = false
    local api = engine.reader()
    local events = if api and api:ping() then api:events { filters = filters } else nil
    if events then
      reachable = true
      watched = events
      -- from here on every change is heard; what happened before this was not
      changed()
      -- ends when Docker goes away, and when another Docker is chosen (`engine.use` closes it)
      while events:next() do
        changed()
      end
      watched = nil
      events:close()
      -- whatever ended them, the windows read again: a Docker that went away is said at once
      changed()
    elseif reachable ~= false then
      reachable = false
      changed()
    end
    -- a Docker that is away is looked for again in a while; one that was just chosen, now
    if not switched then
      task.wait(RETRY)
    end
  end
end

return engine
