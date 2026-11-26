--!strict
-- What this app asks of Docker, and what Docker says turned into the app's own shapes. Which Docker, the clients that
-- talk to it and why a call failed are the container framework's (`vos:docker`).
--
-- A function that does something answers with a plain sentence when it could not (the handler and the tool pass it on
-- as it is) and with nothing when it did what it was asked.
local docker = require("vos:docker")
local stacks = require("./stacks")

local engine = {}

-- what changes the list of containers; the rest of what Docker reports (every health probe, every `exec`) does not
local EVENTS = { "create", "start", "die", "destroy", "pause", "unpause", "rename", "health_status" }

local STATES: { [string]: containers_State } = {
  created = "created",
  running = "running",
  paused = "paused",
  restarting = "restarting",
  removing = "removing",
  exited = "exited",
  dead = "dead",
}

local sentence = docker.sentence

-- Choose the Docker this app manages: an address, or empty for this machine's own.
engine.use = docker.use

local function nameOf(names: { string }?): string
  local first = if names then names[1] else nil
  return if first then (string.gsub(first, "^/", "")) else ""
end

-- the published ports, each once (Docker lists one for IPv4 and one for IPv6), in order
local function portsOf(ports: { vnd_bollard_PortSummary }): { string }
  local seen: { [string]: boolean } = {}
  local published: { { host: number, label: string } } = {}
  for _, port in ports do
    local public = port.PublicPort
    if public then
      local label = `{public}:{port.PrivatePort}` .. (if port.Type == "tcp" then "" else `/{port.Type or ""}`)
      if not seen[label] then
        seen[label] = true
        table.insert(published, { host = public, label = label })
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
local function containerOf(summary: vnd_bollard_ContainerSummary): containers_Container
  local stack, service = stacks.of(summary.Labels or {})
  return {
    id = summary.Id or "",
    name = nameOf(summary.Names),
    image = summary.Image or "",
    state = STATES[summary.State or ""] or "exited",
    status = summary.Status or "",
    ports = portsOf(summary.Ports or {}),
    stack = stack,
    service = service,
  }
end

local function unreachable(status: vos_docker_Status, problem: string): containers_Overview
  status.reachable = false
  status.version = ""
  status.problem = problem
  return { engine = status, containers = {}, stacks = {} }
end

-- Whether Docker can be reached, and what it has. Never fails: a Docker that is away is an answer.
function engine.overview(): containers_Overview
  local status = docker.status()
  local api = if status.reachable then docker.reader() else nil
  if not api then
    return unreachable(status, status.problem)
  end
  local list, list_err = api:list_containers { all = true }
  if not list then
    return unreachable(status, sentence(list_err))
  end
  local containers: { containers_Container } = {}
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
    engine = status,
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
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  return act(api, action, container)
end

-- The same for every container of a stack: starting in the order the services wait for each other, stopping and
-- removing in the reverse. Removing takes the stack's networks with it, as `docker compose down` does; its volumes
-- stay. It goes on past a container that fails, and answers with the first failure.
function engine.stack(action: Action, stack: string): string?
  local api, problem = docker.actor()
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
      { id = summary.Id or "", name = nameOf(summary.Names), service = service, needs = stacks.needs(labels) }
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
    local networks: { vnd_bollard_Network } = api:list_networks { filters = stacks.filter(stack) } or {}
    for _, network in networks do
      if network.Id then
        local _, network_err = api:remove_network(network.Id)
        failure = failure or (if network_err then sentence(network_err) else nil)
      end
    end
  end
  return failure
end

-- What a container was made with. The values of its environment are left out: they often hold passwords.
function engine.details(container: string): (containers_Details?, string?)
  local api, problem = docker.reader()
  if not api then
    return nil, problem
  end
  local found, err = api:inspect_container(container)
  if not found then
    return nil, if err then sentence(err) else "That container is not there any more."
  end

  local command = { found.Path or "" }
  local arguments: { string } = found.Args or {}
  for _, argument in arguments do
    table.insert(command, argument)
  end
  local mounts: { string } = {}
  local mounted: { vnd_bollard_MountPoint } = found.Mounts or {}
  for _, mount in mounted do
    table.insert(mounts, `{mount.Name or mount.Source or ""} -> {mount.Destination or ""}`)
  end
  table.sort(mounts)
  local networks: { string } = {}
  local joined: { [string]: vnd_bollard_EndpointSettings } = (found.NetworkSettings and found.NetworkSettings.Networks)
    or {}
  for name in joined do
    table.insert(networks, name)
  end
  table.sort(networks)
  local environment: { string } = {}
  local variables: { string } = (found.Config and found.Config.Env) or {}
  for _, variable in variables do
    table.insert(environment, string.match(variable, "^[^=]*") :: string)
  end
  table.sort(environment)

  local policy = found.HostConfig and found.HostConfig.RestartPolicy and found.HostConfig.RestartPolicy.Name
  local health = found.State and found.State.Health and found.State.Health.Status
  return {
    container = found.Id or container,
    command = table.concat(command, " "),
    restart = if policy and policy ~= "" then policy else "no",
    health = if health and health ~= "none" then health else "",
    mounts = mounts,
    networks = networks,
    environment = environment,
  },
    nil
end

-- The end of what a container wrote, its output and its errors together in the order Docker has them.
function engine.logs(container: string, tail: number?): (containers_Logs?, string?)
  local logs, problem = docker.logs(container, tail)
  if not logs then
    return nil, problem
  end
  return { container = container, text = logs.text, truncated = logs.truncated }, nil
end

-- Watch Docker for as long as the app is loaded, and call `changed` whenever what `overview` answers may be
-- different. It never returns: run it in a task.
function engine.watch(changed: () -> ())
  docker.watch({ type = { "container" }, event = EVENTS }, changed)
end

return engine
