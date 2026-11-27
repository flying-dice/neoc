--!strict
-- The Containers app's backend: the Docker containers and Compose stacks of a machine. It keeps nothing of its own
-- (the choice of which Docker is kept by `vos:docker`): the containers are Docker's, read each time they are asked for.
-- Every call that changes something answers with the overview as it now stands, so the window that asked draws what
-- Docker has.
--
-- Docker changes without the app (a container exits, someone runs `docker compose up` in a terminal): a task watches
-- Docker's events for as long as the app is loaded and tells the open windows with the `changed` notification, and
-- they read the overview again.
--
-- A person makes containers here too (`_create`, from a spec the form fills in), changes one (`_recreate`: Docker
-- cannot change most of what a container was made with, so it is made again) and removes one. They pull, remove and
-- prune Docker's images, and make, remove and prune its volumes and networks (`_images`, `_volume_create`, ...): each
-- of those calls answers with its list as it now stands. While an image is pulled for them, the windows hear how it
-- goes (`_pulling`).
--
-- An agent can do what a person does most: see what runs (`list_containers`) and what images, volumes and networks
-- there are (`list_images`, `list_volumes`, `list_networks`), read a container's logs (`container_logs`), and start,
-- stop and restart a container or a whole stack. Making, changing and removing are left to the person. A tool
-- that changed something tells the windows itself, so they follow even while the watcher is between two Dockers. It
-- serves on the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`; the page's methods are
-- private (`_overview`, `_start`, ...).
local engine = require("./engine")
local objects = require("./objects")
local task = require("std:task")

-- how long the windows are left alone after being told of a change, in seconds: Docker reports a restart as several
-- events, and a stack as several times that
local QUIET = 0.3

local telling = false
local function changed()
  if telling then
    return
  end
  telling = true
  task.delay(QUIET, function()
    telling = false
    RPC.send_notification("changed")
  end)
end

-- the watcher is the app's own task: it starts at load without holding it up, and stops when the app is unloaded
task.spawn(engine.watch, changed)

RPC.on_request("_overview", function(): containers_OverviewResult
  return engine.overview()
end)

RPC.on_request("_connect", function(params: containers_ConnectParams, ctx: vos_app_Context): containers_ConnectResult
  local problem = engine.use(params.host)
  return if problem then ctx:fail(problem) else engine.overview()
end)

RPC.on_request("_start", function(params: containers_StartParams, ctx: vos_app_Context): containers_StartResult
  local problem = engine.container("start", params.container)
  return if problem then ctx:fail(problem) else engine.overview()
end)

RPC.on_request("_stop", function(params: containers_StopParams, ctx: vos_app_Context): containers_StopResult
  local problem = engine.container("stop", params.container)
  return if problem then ctx:fail(problem) else engine.overview()
end)

RPC.on_request("_restart", function(params: containers_RestartParams, ctx: vos_app_Context): containers_RestartResult
  local problem = engine.container("restart", params.container)
  return if problem then ctx:fail(problem) else engine.overview()
end)

RPC.on_request("_remove", function(params: containers_RemoveParams, ctx: vos_app_Context): containers_RemoveResult
  local problem = engine.remove(params.container, params.force == true, params.volumes == true)
  return if problem then ctx:fail(problem) else engine.overview()
end)

-- how the pull of an image goes, for the windows that wait for it
local function pulling(image: string, status: string, current: number, total: number)
  RPC.send_notification("_pulling", { image = image, status = status, current = current, total = total })
end

RPC.on_request("_create", function(params: containers_CreateParams, ctx: vos_app_Context): containers_CreateResult
  local made, problem = engine.create(params.spec, pulling)
  if not made then
    return ctx:fail(problem or "The container could not be made.")
  end
  return { container = made, overview = engine.overview() }
end)

RPC.on_request("_recreate", function(params: containers_RecreateParams, ctx: vos_app_Context): containers_RecreateResult
  local made, problem = engine.recreate(params.container, params.spec, pulling)
  if not made then
    return ctx:fail(problem or "The container could not be made again.")
  end
  return { container = made, overview = engine.overview() }
end)

-- the images, volumes and networks: each change answers with its list as it now stands
local function images(ctx: vos_app_Context): { containers_Image }
  local list, problem = objects.images()
  return list or ctx:fail(problem or "Docker did not list its images.")
end

local function volumes(ctx: vos_app_Context): { containers_Volume }
  local list, problem = objects.volumes()
  return list or ctx:fail(problem or "Docker did not list its volumes.")
end

local function networks(ctx: vos_app_Context): { containers_Network }
  local list, problem = objects.networks()
  return list or ctx:fail(problem or "Docker did not list its networks.")
end

RPC.on_request("_networks", function(_params: {}, ctx: vos_app_Context): containers_NetworksResult
  return networks(ctx)
end)

RPC.on_request("_volumes", function(_params: {}, ctx: vos_app_Context): containers_VolumesResult
  return volumes(ctx)
end)

RPC.on_request(
  "_volume_create",
  function(params: containers_VolumeCreateParams, ctx: vos_app_Context): containers_VolumeCreateResult
    local problem = objects.create_volume(params.name, params.driver, params.labels)
    return if problem then ctx:fail(problem) else volumes(ctx)
  end
)

RPC.on_request(
  "_volume_remove",
  function(params: containers_VolumeRemoveParams, ctx: vos_app_Context): containers_VolumeRemoveResult
    local problem = objects.remove_volume(params.volume)
    return if problem then ctx:fail(problem) else volumes(ctx)
  end
)

RPC.on_request("_volumes_prune", function(_params: {}, ctx: vos_app_Context): containers_VolumesPruneResult
  local removed, space, problem = objects.prune_volumes()
  if not removed then
    return ctx:fail(problem or "Docker did not prune its volumes.")
  end
  return { volumes = volumes(ctx), removed = removed, space = space or 0 }
end)

RPC.on_request(
  "_network_create",
  function(params: containers_NetworkCreateParams, ctx: vos_app_Context): containers_NetworkCreateResult
    local problem = objects.create_network(params)
    return if problem then ctx:fail(problem) else networks(ctx)
  end
)

RPC.on_request(
  "_network_remove",
  function(params: containers_NetworkRemoveParams, ctx: vos_app_Context): containers_NetworkRemoveResult
    local problem = objects.remove_network(params.network)
    return if problem then ctx:fail(problem) else networks(ctx)
  end
)

RPC.on_request("_images", function(_params: {}, ctx: vos_app_Context): containers_ImagesResult
  return images(ctx)
end)

RPC.on_request(
  "_image_pull",
  function(params: containers_ImagePullParams, ctx: vos_app_Context): containers_ImagePullResult
    local problem = engine.pull(params.image, pulling)
    return if problem then ctx:fail(problem) else images(ctx)
  end
)

RPC.on_request(
  "_image_remove",
  function(params: containers_ImageRemoveParams, ctx: vos_app_Context): containers_ImageRemoveResult
    local problem = objects.remove_image(params.image, params.force == true)
    return if problem then ctx:fail(problem) else images(ctx)
  end
)

RPC.on_request("_images_prune", function(_params: {}, ctx: vos_app_Context): containers_ImagesPruneResult
  local removed, space, problem = objects.prune_images()
  if not removed then
    return ctx:fail(problem or "Docker did not prune its images.")
  end
  return { images = images(ctx), removed = removed, space = space or 0 }
end)

RPC.on_request(
  "_stack_start",
  function(params: containers_StackStartParams, ctx: vos_app_Context): containers_StackStartResult
    local problem = engine.stack("start", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end
)

RPC.on_request(
  "_stack_stop",
  function(params: containers_StackStopParams, ctx: vos_app_Context): containers_StackStopResult
    local problem = engine.stack("stop", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end
)

RPC.on_request(
  "_stack_restart",
  function(params: containers_StackRestartParams, ctx: vos_app_Context): containers_StackRestartResult
    local problem = engine.stack("restart", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end
)

RPC.on_request(
  "_stack_remove",
  function(params: containers_StackRemoveParams, ctx: vos_app_Context): containers_StackRemoveResult
    local problem = engine.stack("remove", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end
)

RPC.on_request("_details", function(params: containers_DetailsParams, ctx: vos_app_Context): containers_DetailsResult
  local details, problem = engine.details(params.container)
  return details or ctx:fail(problem or "That container is not there any more.")
end)

RPC.on_request("_logs", function(params: containers_LogsParams, ctx: vos_app_Context): containers_LogsResult
  local logs, problem = engine.logs(params.container, params.tail)
  return logs or ctx:fail(problem or "That container is not there any more.")
end)

RPC.on_request(
  "list_containers",
  function(params: containers_ListContainersParams, ctx: vos_app_Context): containers_ListContainersResult
    local overview = engine.overview()
    if not overview.engine.reachable then
      return ctx:fail(overview.engine.problem)
    end
    local containers: { containers_Listed } = {}
    for _, container in overview.containers do
      if params.stack == nil or container.stack == params.stack then
        table.insert(containers, {
          name = container.name,
          image = container.image,
          state = container.state,
          status = container.status,
          ports = container.ports,
          stack = container.stack,
        })
      end
    end
    return { docker = overview.engine.version, containers = containers }
  end
)

RPC.on_request(
  "container_logs",
  function(params: containers_ContainerLogsParams, ctx: vos_app_Context): containers_ContainerLogsResult
    local logs, problem = engine.logs(params.container, params.lines)
    if not logs then
      return ctx:fail(problem or "There is no such container.")
    end
    return { container = params.container, logs = logs.text, truncated = logs.truncated }
  end
)

RPC.on_request("list_images", function(_params: {}, ctx: vos_app_Context): containers_ListImagesResult
  return { images = images(ctx) }
end)

RPC.on_request("list_volumes", function(_params: {}, ctx: vos_app_Context): containers_ListVolumesResult
  return { volumes = volumes(ctx) }
end)

RPC.on_request("list_networks", function(_params: {}, ctx: vos_app_Context): containers_ListNetworksResult
  return { networks = networks(ctx) }
end)

-- the tools that act on one container, and on one stack: each answers with what it did
RPC.on_request(
  "start_container",
  function(params: containers_StartContainerParams, ctx: vos_app_Context): containers_StartContainerResult
    local problem = engine.container("start", params.container)
    if problem then
      return ctx:fail(problem)
    end
    changed()
    return { container = params.container, started = true }
  end
)

RPC.on_request(
  "stop_container",
  function(params: containers_StopContainerParams, ctx: vos_app_Context): containers_StopContainerResult
    local problem = engine.container("stop", params.container)
    if problem then
      return ctx:fail(problem)
    end
    changed()
    return { container = params.container, stopped = true }
  end
)

RPC.on_request(
  "restart_container",
  function(params: containers_RestartContainerParams, ctx: vos_app_Context): containers_RestartContainerResult
    local problem = engine.container("restart", params.container)
    if problem then
      return ctx:fail(problem)
    end
    changed()
    return { container = params.container, restarted = true }
  end
)

RPC.on_request(
  "start_stack",
  function(params: containers_StartStackParams, ctx: vos_app_Context): containers_StartStackResult
    local problem = engine.stack("start", params.stack)
    if problem then
      return ctx:fail(problem)
    end
    changed()
    return { stack = params.stack, started = true }
  end
)

RPC.on_request(
  "stop_stack",
  function(params: containers_StopStackParams, ctx: vos_app_Context): containers_StopStackResult
    local problem = engine.stack("stop", params.stack)
    if problem then
      return ctx:fail(problem)
    end
    changed()
    return { stack = params.stack, stopped = true }
  end
)
