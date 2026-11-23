--!strict
-- Docker, as this app uses it: the one place that talks to the Docker daemon (through `vnd:bollard`) and turns what
-- Docker says into the app's own shapes. Which Docker is the person's choice (`engine.use`), kept in the app's
-- settings; with none chosen it is this machine's own.
--
-- A function that does something answers with a plain sentence when it could not (the handler and the tool pass it on
-- as it is) and with nothing when it did what it was asked.
local app = require("vos:app")
local bollard = require("vnd:bollard")
local rpc = require("../.app/rpc")
local stacks = require("./stacks")
local task = require("std:task")
local text = require("./text")

local engine = {}

-- the setting: where Docker listens; absent for this machine's own
local HOST = "host"
-- How long a call may wait for Docker's answer, in seconds. Reading is quick or Docker is not answering (a Docker
-- that is starting, or paused, takes the connection and says nothing): the window should hear of that soon. Acting
-- takes as long as the container does: Docker gives one ten seconds to stop before it kills it.
local READ_TIMEOUT = 5
local ACT_TIMEOUT = 90
-- how often a Docker that is away is looked for again, in seconds
local RETRY = 5
-- the most of a container's output that is read and sent, in bytes
local LOG_LIMIT = 256 * 1024
local DEFAULT_TAIL = 200
-- what changes the list of containers; the rest of what Docker reports (every health probe, every `exec`) does not
local EVENTS = { "create", "start", "die", "destroy", "pause", "unpause", "rename", "health_status" }

local STATES: { [string]: rpc.State } = {
  created = "created",
  running = "running",
  paused = "paused",
  restarting = "restarting",
  removing = "removing",
  exited = "exited",
  dead = "dead",
}

-- the clients of the chosen Docker, one to read with and one to act with (they differ in how long they wait), made
-- on first use and again after the choice changes
type Clients = { reading: vnd_bollard_Client, acting: vnd_bollard_Client }
local clients: Clients? = nil
-- the events the watcher is reading, so that a change of Docker can end them
local watched: vnd_bollard_Stream? = nil
-- another Docker was chosen since the watcher last looked: it looks for the new one at once
local switched = false

function engine.host(): string
  local host = app.kv:get(HOST)
  return if type(host) == "string" then host else ""
end

-- Why a call failed, as a sentence for the person. Docker's own message is one already ("No such container: web");
-- no Docker at all is said in the app's words.
local function sentence(err: string?): string
  local kind = bollard.error_kind(err)
  if kind == "connect" then
    local host = engine.host()
    return if host == ""
      then "Docker is not running on this machine, or cannot be reached."
      else `Docker cannot be reached at {host}.`
  elseif kind == "timed_out" then
    return "Docker is not answering. It may still be starting, or be paused."
  end
  local message = bollard.error_message(err)
  return if message and message ~= "" then message else err or "Docker refused, and did not say why."
end

local function connected(): (Clients?, string?)
  if clients then
    return clients, nil
  end
  local host = if engine.host() == "" then nil else engine.host()
  local reading, err = bollard.connect { host = host, timeout = READ_TIMEOUT }
  if not reading then
    return nil, sentence(err)
  end
  local acting, acting_err = bollard.connect { host = host, timeout = ACT_TIMEOUT }
  if not acting then
    return nil, sentence(acting_err)
  end
  clients = { reading = reading, acting = acting }
  return clients, nil
end

-- the client that reads what Docker has
local function docker(): (vnd_bollard_Client?, string?)
  local both, problem = connected()
  return if both then both.reading else nil, problem
end

-- the client that changes it
local function actor(): (vnd_bollard_Client?, string?)
  local both, problem = connected()
  return if both then both.acting else nil, problem
end

-- Choose the Docker this app manages: an address, or empty for this machine's own. The address is only checked for
-- being one (a Docker that is not running there is said by the overview, and may start later).
function engine.use(host: string): string?
  host = string.match(host, "^%s*(.-)%s*$") :: string
  if host ~= "" then
    if
      not (string.match(host, "^unix:///") or string.match(host, "^tcp://[^/]") or string.match(host, "^https?://[^/]"))
    then
      return "The address must start with unix:///, tcp:// or http://, or be empty for this machine's own Docker."
    end
    -- a socket that is not there is a fine choice (Docker may not be running yet): only an address that is none is not
    local _, err = bollard.connect { host = host }
    if err and bollard.error_kind(err) ~= "connect" then
      return sentence(err)
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

local function nameOf(names: any): string
  local first = if type(names) == "table" then names[1] else nil
  return if type(first) == "string" then (string.gsub(first, "^/", "")) else ""
end

-- the published ports, each once (Docker lists one for IPv4 and one for IPv6), in order
local function portsOf(ports: any): { string }
  local seen: { [string]: boolean } = {}
  local published: { { host: number, label: string } } = {}
  for _, port in ports or {} do
    if port.PublicPort then
      local label = `{port.PublicPort}:{port.PrivatePort}` .. (if port.Type == "tcp" then "" else `/{port.Type}`)
      if not seen[label] then
        seen[label] = true
        table.insert(published, { host = port.PublicPort, label = label })
      end
    end
  end
  table.sort(published, function(a, b)
    return if a.host == b.host then a.label < b.label else a.host < b.host
  end)
  local labels: { string } = {}
  for _, port in published do
    table.insert(labels, port.label)
  end
  return labels
end

-- a container, from an entry of Docker's list
local function containerOf(summary: any): rpc.Container
  local stack, service = stacks.of(summary.Labels or {})
  return {
    id = summary.Id,
    name = nameOf(summary.Names),
    image = summary.Image or "",
    state = STATES[summary.State] or "exited",
    status = summary.Status or "",
    ports = portsOf(summary.Ports),
    stack = stack,
    service = service,
  }
end

local function unreachable(problem: string): rpc.Overview
  return {
    engine = { reachable = false, version = "", host = engine.host(), problem = problem },
    containers = {},
    stacks = {},
  }
end

-- Whether Docker can be reached, and what it has. Never fails: a Docker that is away is an answer.
function engine.overview(): rpc.Overview
  local api, problem = docker()
  if not api then
    return unreachable(problem or "")
  end
  local version, version_err = api:version()
  if not version then
    return unreachable(sentence(version_err))
  end
  local list, list_err = api:list_containers { all = true }
  if not list then
    return unreachable(sentence(list_err))
  end
  local containers: { rpc.Container } = {}
  local folders: { [string]: string } = {}
  for _, summary in list do
    local container = containerOf(summary)
    table.insert(containers, container)
    if container.stack ~= "" and not folders[container.stack] then
      folders[container.stack] = stacks.folder(summary.Labels or {})
    end
  end
  table.sort(containers, function(a, b)
    return if a.name == b.name then a.id < b.id else a.name < b.name
  end)
  return {
    engine = { reachable = true, version = version.Version or "", host = engine.host(), problem = "" },
    containers = containers,
    stacks = stacks.summarise(containers, folders),
  }
end

export type Action = "start" | "stop" | "restart" | "remove"

-- one action on one container, by id or name
local function act(api: vnd_bollard_Client, action: Action, container: string): string?
  local _, err
  if action == "start" then
    _, err = api:start_container(container)
  elseif action == "stop" then
    _, err = api:stop_container(container)
  elseif action == "restart" then
    _, err = api:restart_container(container)
  else
    -- running or not: the person was asked first. Named volumes are not the container's, and stay
    _, err = api:remove_container(container, { force = true })
  end
  return if err then sentence(err) else nil
end

function engine.container(action: Action, container: string): string?
  local api, problem = actor()
  if not api then
    return problem
  end
  return act(api, action, container)
end

-- The same for every container of a stack: starting in the order the services wait for each other, stopping and
-- removing in the reverse. Removing takes the stack's networks with it, as `docker compose down` does; its volumes
-- stay. It goes on past a container that fails, and answers with the first failure.
function engine.stack(action: Action, stack: string): string?
  local api, problem = actor()
  if not api then
    return problem
  end
  local list, err = api:list_containers { all = true, filters = stacks.filter(stack) }
  if not list then
    return sentence(err)
  end
  local members: { stacks.Member } = {}
  for _, summary in list do
    local labels = summary.Labels or {}
    local _, service = stacks.of(labels)
    table.insert(
      members,
      { id = summary.Id, name = nameOf(summary.Names), service = service, needs = stacks.needs(labels) }
    )
  end
  if #members == 0 then
    return "There is no such stack."
  end
  local ordered = stacks.order(members)

  local failure: string? = nil
  local function each(step: Action, backwards: boolean)
    for i = if backwards then #ordered else 1, if backwards then 1 else #ordered, if backwards then -1 else 1 do
      failure = failure or act(api, step, ordered[i].id)
    end
  end
  if action == "start" then
    each("start", false)
  elseif action == "restart" then
    each("stop", true)
    each("start", false)
  else
    each(action, true)
  end

  if action == "remove" then
    local networks = api:list_networks { filters = stacks.filter(stack) }
    for _, network in networks or {} do
      local _, network_err = api:remove_network(network.Id)
      failure = failure or (if network_err then sentence(network_err) else nil)
    end
  end
  return failure
end

-- What a container was made with. The values of its environment are left out: they often hold passwords.
function engine.details(container: string): (rpc.Details?, string?)
  local api, problem = docker()
  if not api then
    return nil, problem
  end
  local found, err = api:inspect_container(container)
  if not found then
    return nil, if err then sentence(err) else "That container is not there any more."
  end

  local command = { found.Path or "" }
  for _, argument in found.Args or {} do
    table.insert(command, argument)
  end
  local mounts: { string } = {}
  for _, mount in found.Mounts or {} do
    table.insert(mounts, `{mount.Name or mount.Source or ""} -> {mount.Destination or ""}`)
  end
  table.sort(mounts)
  local networks: { string } = {}
  for name in (found.NetworkSettings and found.NetworkSettings.Networks) or {} do
    table.insert(networks, name)
  end
  table.sort(networks)
  local environment: { string } = {}
  for _, variable in (found.Config and found.Config.Env) or {} do
    table.insert(environment, string.match(variable, "^[^=]*") :: string)
  end
  table.sort(environment)

  local policy = found.HostConfig and found.HostConfig.RestartPolicy and found.HostConfig.RestartPolicy.Name
  local health = found.State and found.State.Health and found.State.Health.Status
  return {
    container = found.Id,
    command = table.concat(command, " "),
    restart = if type(policy) == "string" and policy ~= "" then policy else "no",
    health = if type(health) == "string" and health ~= "none" then health else "",
    mounts = mounts,
    networks = networks,
    environment = environment,
  },
    nil
end

-- The end of what a container wrote, its output and its errors together in the order Docker has them.
function engine.logs(container: string, tail: number?): (rpc.Logs?, string?)
  local api, problem = docker()
  if not api then
    return nil, problem
  end
  local lines = tail or DEFAULT_TAIL
  local stream, err = api:logs(container, { tail = lines })
  if not stream then
    return nil, sentence(err)
  end
  local pieces: { string } = {}
  while true do
    local piece, piece_err = stream:next()
    if piece_err then
      return nil, sentence(piece_err)
    end
    if not piece then
      break
    end
    table.insert(pieces, piece.data)
  end
  local written, cut = text.tail(table.concat(pieces), LOG_LIMIT)
  local _, breaks = string.gsub(written, "\n", "")
  return { container = container, text = written, truncated = cut or breaks >= lines }, nil
end

-- Watch Docker for as long as the app is loaded, and call `changed` whenever what `overview` answers may be
-- different: a container was made, started, stopped or removed, Docker itself came or went, or the watcher was not
-- listening for a while (Docker does not repeat what it reported to nobody). It never returns: run it in a task.
function engine.watch(changed: () -> ())
  local reachable: boolean? = nil
  while true do
    switched = false
    local api = docker()
    local events = if api and api:ping() then api:events { filters = { type = "container", event = EVENTS } } else nil
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
