--!strict
-- What this app asks of Docker, and what Docker says turned into the app's own shapes. Which Docker, the clients that
-- talk to it and why a call failed are the container framework's (`vos:docker`).
--
-- A function that does something answers with a plain sentence when it could not (the handler and the tool pass it on
-- as it is) and with nothing when it did what it was asked.
local docker = require("vos:docker")
local spec = require("./spec")
local stacks = require("./stacks")
local time = require("std:time")
local tracing = require("lib:tracing")

local engine = {}

-- what changes the lists the windows show: a container's, an image's, a volume's and a network's comings and goings.
-- The rest of what Docker reports (every health probe, every `exec`, a volume mounted as its container starts) does not
local TYPES = { "container", "image", "volume", "network" }
local EVENTS = {
  "create",
  "start",
  "die",
  "destroy",
  "pause",
  "unpause",
  "rename",
  "health_status",
  "pull",
  "tag",
  "untag",
  "delete",
  "import",
  "load",
  "prune",
  "connect",
  "disconnect",
  "remove",
}

local STATES: { [string]: containers_State } = {
  created = "created",
  running = "running",
  paused = "paused",
  restarting = "restarting",
  removing = "removing",
  exited = "exited",
  dead = "dead",
}

-- how often the windows are told how a pull goes, at most, in milliseconds
local PULL_EVERY = 250

local sentence = docker.sentence

-- Hears how the pull of an image goes: Docker's word for the step it is at, and how many bytes of the image's layers
-- are downloaded of how many (0 while Docker has not said).
export type Progress = (image: string, status: string, current: number, total: number) -> ()
-- what is downloaded of a layer of an image, in bytes
type Layer = { current: number, total: number }

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

-- What a container was made with: what it runs and is mounted as Docker has it, and its spec, for the form that
-- changes or copies it. Its environment's values are in the spec: the window hides them until they are asked for.
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
    local line = `{mount.Name or mount.Source or ""} -> {mount.Destination or ""}`
    table.insert(mounts, if mount.RW == false then `{line} (read-only)` else line)
  end
  table.sort(mounts)
  local networks: { string } = {}
  local joined: { [string]: vnd_bollard_EndpointSettings } = (found.NetworkSettings and found.NetworkSettings.Networks)
    or {}
  for name in joined do
    table.insert(networks, name)
  end
  table.sort(networks)

  local health = found.State and found.State.Health and found.State.Health.Status
  return {
    container = found.Id or container,
    command = table.concat(command, " "),
    health = if health and health ~= "none" then health else "",
    mounts = mounts,
    networks = networks,
    spec = spec.of(found),
  },
    nil
end

-- Pull an image from its registry, newer or not; `progress` hears how the pull goes.
local function pull(image: string, progress: Progress): string?
  local puller, unreachable = docker.puller()
  if not puller then
    return unreachable
  end
  local steps = puller:pull_image(spec.tagged(image))
  -- what is downloaded of each layer, by its id
  local layers: { [string]: Layer } = {}
  local told: typeof(time.now())? = nil
  local function tell(status: string)
    if told and told:elapsed():as_millis() < PULL_EVERY then
      return
    end
    told = time.now()
    local current, total = 0, 0
    for _, layer in layers do
      current += layer.current
      total += layer.total
    end
    progress(image, status, current, total)
  end
  -- the pull runs as its progress is read: an image that cannot be pulled is the error of a step
  while true do
    local step, step_err = steps:next()
    local refused = step and step.errorDetail and step.errorDetail.message
    if step_err or refused then
      return `The image {image} could not be pulled: {if refused then refused else sentence(step_err)}`
    end
    if not step then
      return nil
    end
    local status: string = step.status or ""
    if step.id then
      local layer: Layer = layers[step.id] or { current = 0, total = 0 }
      layers[step.id] = layer
      local detail = step.progressDetail
      -- extracting has a progress of its own, which is not the download's
      if status == "Downloading" and detail and detail.total then
        layer.total = detail.total
        layer.current = detail.current or 0
      elseif status == "Download complete" or status == "Pull complete" then
        layer.current = layer.total
      end
    end
    tell(status)
  end
end

-- Have the image, pulling it when Docker does not have it; `progress` hears how the pull goes.
local function pulled(api: vnd_bollard_Client, image: string, progress: Progress): string?
  local images, err = api:list_images { filters = { reference = { spec.tagged(image) } } }
  if not images then
    return sentence(err)
  end
  if #images > 0 then
    return nil
  end
  return pull(image, progress)
end

-- Pull an image by its name, as `docker pull` does: whether Docker has it or not, for a newer one of that name.
-- Answers with why it could not be pulled, or with nothing.
function engine.pull(image: string, progress: Progress): string?
  local name = spec.trim(image)
  if name == "" then
    return "Say which image to pull, like nginx:1.27."
  end
  if not spec.isImage(name) then
    return `{name} is not the name of an image. An image is named like nginx, nginx:1.27 or ghcr.io/owner/app:1.0.`
  end
  return pull(name, progress)
end

-- Make a container from a spec Docker can be asked for, named `name` (nil: Docker chooses), and start it when the
-- spec says so. One that could not be started is removed again: nothing is left of what failed.
local function make(api: vnd_bollard_Client, asked: spec.Spec, name: string?): (string?, string?)
  local made, err = api:create_container(spec.body(asked), { name = name })
  if not made then
    return nil, sentence(err)
  end
  if asked.start then
    local _, start_err = api:start_container(made.Id)
    if start_err then
      local _, left_err = api:remove_container(made.Id, { force = true, volumes = true })
      if left_err then
        tracing.warn(
          "could not remove the container that failed to start",
          { error = sentence(left_err), container = made.Id }
        )
      end
      return nil, sentence(start_err)
    end
  end
  return made.Id, nil
end

-- Make a container from a spec: checked, its image pulled when Docker does not have it, made, and started when the
-- spec says so. Answers with its id, or with why it could not be made.
function engine.create(asked: spec.Spec, progress: Progress): (string?, string?)
  local wrong = spec.refused(asked)
  if wrong then
    return nil, wrong
  end
  local api, problem = docker.actor()
  if not api then
    return nil, problem
  end
  local missing = pulled(api, spec.trim(asked.image), progress)
  if missing then
    return nil, missing
  end
  local name = spec.trim(asked.name)
  return make(api, asked, if name == "" then nil else name)
end

-- Make a container again from a changed spec, as Docker cannot change one: the old one is stopped and removed (one
-- name is one container's), and the new one made in its place, by the name the spec gives or else the old one's.
-- Everything that can be refused before is (the spec, the image), so the old one is touched only for what Docker alone
-- can refuse; when it does, the old one is made again from its own settings, and started when it ran. Its anonymous
-- volumes and named ones stay. Answers with the new one's id, or with why it could not be made.
function engine.recreate(container: string, asked: spec.Spec, progress: Progress): (string?, string?)
  local wrong = spec.refused(asked)
  if wrong then
    return nil, wrong
  end
  local api, problem = docker.actor()
  if not api then
    return nil, problem
  end
  local old, err = api:inspect_container(container)
  if not old or not old.Id then
    return nil, if err then sentence(err) else "That container is not there any more."
  end
  local missing = pulled(api, spec.trim(asked.image), progress)
  if missing then
    return nil, missing
  end
  local id = old.Id :: string
  local ran = old.State ~= nil and old.State.Running == true
  local name = string.gsub(old.Name or "", "^/", "")
  local wanted = spec.trim(asked.name)

  -- the old one steps aside, stopped and under another name, until the new one runs: if the new one cannot be made, the
  -- old one gets its name back and runs again as it was, everything in it kept
  if ran then
    local _, stop_err = api:stop_container(id)
    if stop_err then
      return nil, sentence(stop_err)
    end
  end
  local aside = `{name}-old-{string.sub(id, 1, 12)}`
  local _, rename_err = api:rename_container(id, aside)
  if rename_err then
    if ran then
      local _, again_err = api:start_container(id)
      if again_err then
        tracing.warn("could not start the old container again", { error = sentence(again_err), container = id })
      end
    end
    return nil, sentence(rename_err)
  end

  local made, failure = make(api, asked, if wanted == "" then name else wanted)
  if not made then
    local _, back_err = api:rename_container(id, name)
    if back_err then
      return nil,
        `{failure} The old container is stopped as {aside}: it could not have its name back ({sentence(back_err)}).`
    end
    if ran then
      local _, again_err = api:start_container(id)
      if again_err then
        tracing.warn("could not start the old container again", { error = sentence(again_err), container = id })
      end
    end
    return nil, failure
  end
  -- its named volumes stay; what was only in the old container goes with it. Should Docker refuse, the new one runs all
  -- the same, and the old one is left stopped as `aside`, in the list, for the person to remove
  local _, old_err = api:remove_container(id, { force = true })
  if old_err then
    tracing.warn(
      "could not remove the old container, left stopped",
      { error = sentence(old_err), container = id, aside = aside }
    )
  end
  return made, nil
end

-- Remove a container: one that runs only when `force`, which kills it first. Its anonymous volumes go with it when
-- `volumes`; its named volumes always stay.
function engine.remove(container: string, force: boolean, volumes: boolean): string?
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  local _, err = api:remove_container(container, { force = force, volumes = volumes })
  return if err then sentence(err) else nil
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
  docker.watch({ type = TYPES, event = EVENTS }, changed)
end

return engine
