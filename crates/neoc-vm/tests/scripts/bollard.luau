--!strict
-- vnd:bollard as a script sees it: its answers (bollard.json), then what it asserts. All of it without a Docker
-- daemon: connecting sends nothing, so a client is made; what a call answers needs a daemon and is left to the
-- module's own tests (a fake daemon in the process, and the real one when asked for).
local bollard = require("vnd:bollard")
local out = {}

local function refused(call: () -> ...unknown): boolean
  local ok, value, err = pcall(call)
  return not ok or (value == nil and err ~= nil)
end

-- a client is made without a daemon: port 1 is one nothing listens on
local connected, connect_err = bollard.connect { host = "tcp://127.0.0.1:1", timeout = 5 }
out.connect = { type(connected), connect_err == nil } :: { any }
local docker = assert(connected)

-- the methods both have
local methods = {}
for _, name in
  {
    "ping",
    "version",
    "list_containers",
    "create_container",
    "start_container",
    "stop_container",
    "restart_container",
    "kill_container",
    "pause_container",
    "unpause_container",
    "remove_container",
    "logs",
    "exec",
    "list_images",
    "pull_image",
    "remove_image",
    "remove_volume",
    "create_network",
    "remove_network",
    "events",
  }
do
  table.insert(methods, type((docker :: any)[name]))
end
out.methods = methods

-- nothing answers: the first call says so
local pong, ping_err = docker:ping()
out.nothing_answers = { pong == nil, ping_err ~= nil }

-- refused before anything is sent
out.refused = {
  refused(function()
    return bollard.connect({ hots = "tcp://127.0.0.1:1" } :: any)
  end),
  refused(function()
    return bollard.connect { host = "tcp://127.0.0.1:1", timeout = -1 }
  end),
  refused(function()
    return bollard.connect { host = "gopher://127.0.0.1" }
  end),
  refused(function()
    return docker:start_container("../images/json")
  end),
  refused(function()
    return docker:stop_container("web", { signal = "TERM; rm" })
  end),
  refused(function()
    return docker:list_containers({ al = true } :: any)
  end),
  refused(function()
    return docker:remove_image("../containers/web")
  end),
  refused(function()
    return docker:exec("web", {} :: any)
  end),
}

-- what it asserts
do
  local function raised(call: () -> ()): string
    local ok, err = pcall(call :: () -> unknown)
    assert(not ok, "accepted")
    return tostring(err)
  end

  -- the functions that took an error text apart are gone: the error is a table
  assert((bollard :: any).error_kind == nil and (bollard :: any).error_message == nil)

  -- connecting sends nothing; what cannot be a client is a typed error
  local docker = assert(bollard.connect { host = "tcp://127.0.0.1:1", timeout = 5 })
  local none, scheme = bollard.connect { host = "gopher://127.0.0.1" }
  assert(none == nil and scheme ~= nil)
  assert(scheme.kind == "invalid_input" and scheme.op == "vnd:bollard.connect")
  assert(tostring(scheme) == "vnd:bollard.connect: invalid_input: " .. scheme.message)
  local _, timeout = bollard.connect { host = "tcp://127.0.0.1:1", timeout = 0 }
  assert(timeout ~= nil and timeout.kind == "invalid_input")
  if timeout.kind == "invalid_input" then
    assert(timeout.reason == 'option "timeout" must be more than zero seconds')
  end

  -- nothing listens on port 1: the first call's error, with its kind and Docker's (here the client's) words
  local pong, unreachable = docker:ping()
  assert(pong == nil and unreachable ~= nil)
  assert(unreachable.kind == "connect" and unreachable.op == "vnd:bollard.Client:ping")
  if unreachable.kind == "connect" then
    assert(unreachable.reason:find("the Docker daemon could not be reached", 1, true))
  end
  local list, list_err = docker:list_containers { all = true, filters = { status = { "running" } } }
  assert(list == nil and list_err ~= nil and list_err.kind == "connect")
  -- inspect_container is (nil, nil) only when Docker says there is no such container: no daemon is an error
  local inspected, inspect_err = docker:inspect_container("web")
  assert(inspected == nil and inspect_err ~= nil and inspect_err.kind == "connect")
  assert(inspect_err.op == "vnd:bollard.Client:inspect_container")
  local info, info_err = docker:info()
  assert(info == nil and info_err ~= nil and info_err.kind == "connect")
  local volumes, volumes_err = docker:list_volumes()
  assert(volumes == nil and volumes_err ~= nil and volumes_err.kind == "connect")
  local networks, networks_err = docker:list_networks { filters = { driver = { "bridge" } } }
  assert(networks == nil and networks_err ~= nil and networks_err.kind == "connect")
  local no_stats, stats_err = docker:stats("web")
  assert(no_stats == nil and stats_err ~= nil and stats_err.kind == "connect")
  local no_volume, volume_err = docker:create_volume { Name = "data" }
  assert(no_volume == nil and volume_err ~= nil and volume_err.kind == "connect")

  -- a stream is returned as it is: the failure is its first next()'s, and then it is over
  local logs = docker:logs("web", { tail = 10 })
  local piece, logs_err = logs:next()
  assert(piece == nil and logs_err ~= nil and logs_err.kind == "connect")
  assert(logs_err.op == "vnd:bollard.Client:logs")
  local after, after_err = logs:next()
  assert(after == nil and after_err == nil)
  local events = docker:events { filters = { type = { "container" } } }
  events:close()
  local event, event_err = events:next()
  assert(event == nil and event_err == nil)
  assert(not pcall(function()
    for _ in docker:pull_image("nginx:1.27"):items() do
    end
  end))

  -- names are checked text: what is not one is refused with its place, before anything is sent
  local place = "vnd:bollard.Client:start_container: argument #1 'name': the container must be a name or an id"
  assert(raised(function()
    docker:start_container("../images/json")
  end):find(place, 1, true))
  assert(raised(function()
    docker:remove_volume("a/b")
  end):find("argument #1 'name': the volume must be a name", 1, true))
  assert(raised(function()
    docker:remove_network("")
  end):find("argument #1 'name': the network must be a name", 1, true))
  assert(raised(function()
    docker:pull_image("nginx?tag=x")
  end):find("argument #1 'image': the image must be a reference", 1, true))
  assert(raised(function()
    docker:kill_container("web", { signal = "HUP; rm" })
  end):find("a signal is its name or its number", 1, true))
  assert(
    raised(function()
      docker:inspect_container("web?size=1")
    end):find(
      "vnd:bollard.Client:inspect_container: argument #1 'name': the container must be a name or an id",
      1,
      true
    )
  )
  assert(raised(function()
    docker:stats("")
  end):find("argument #1 'name': the container must be a name or an id", 1, true))

  -- options are the call's own: an unknown one, a wrong type, a filter that is not a list
  assert(raised(function()
    docker:list_containers({ al = true } :: any)
  end):find("unknown field `al`", 1, true))
  assert(raised(function()
    docker:list_containers({ filters = { status = "running" } } :: any)
  end):find("filters", 1, true))
  assert(raised(function()
    docker:logs("web", { tail = -1 })
  end):find("argument #2 'opts'", 1, true))
  assert(raised(function()
    docker:list_volumes({ filter = { name = { "data" } } } :: any)
  end):find("unknown field `filter`", 1, true))
  assert(raised(function()
    docker:list_networks({ filters = { name = "bridge" } } :: any)
  end):find("filters", 1, true))
  assert(raised(function()
    bollard.connect({ tls = { ca = "ca.pem" } } :: any)
  end):find("vnd:bollard.connect: argument #1 'opts'", 1, true))
  assert(raised(function()
    bollard.connect({ tls_ca = "ca.pem" } :: any)
  end):find("unknown field `tls_ca`", 1, true))

  -- Docker's body is typed: a field it does not have, or of the wrong type, is refused with its path
  assert(raised(function()
    docker:create_container({ Image = "nginx", name = "web" } :: any)
  end):find("argument #1 'body': unknown field `name`", 1, true))
  assert(raised(function()
    docker:create_container({ Image = "nginx", HostConfig = { Memroy = 1 } } :: any)
  end):find("HostConfig: unknown field `Memroy`", 1, true))
  assert(raised(function()
    docker:create_container({ Image = { "nginx" } } :: any)
  end):find("Image: expected a string", 1, true))
  assert(raised(function()
    docker:create_container({ Image = "nginx" }, { name = "a/b" })
  end):find("the container must be a name or an id", 1, true))
  assert(raised(function()
    docker:create_network({ Name = true } :: any)
  end):find("Name: expected a string", 1, true))
  assert(raised(function()
    docker:create_volume({ Name = true } :: any)
  end):find("Name: expected a string", 1, true))
  assert(raised(function()
    docker:create_volume({ Name = "data", Lables = {} } :: any)
  end):find("argument #1 'body': unknown field `Lables`", 1, true))

  -- an empty command is the call's own refusal
  local ran, no_command = docker:exec("web", {})
  assert(ran == nil and no_command ~= nil and no_command.kind == "invalid_input")

  -- what a script writes type-checks; it is not run here, because it needs a daemon
  local function _usage()
    local local_docker = assert(bollard.connect())
    local version = assert(local_docker:version())
    local _api: string? = version.ApiVersion
    local made, create_err = local_docker:create_container({
      Image = "nginx:1.27",
      Env = { "A=1" },
      Labels = { app = "web" },
      ExposedPorts = { ["80/tcp"] = {} },
      HostConfig = {
        Memory = 64 * 1024 * 1024,
        Binds = { "/srv/www:/usr/share/nginx/html:ro" },
        PortBindings = { ["80/tcp"] = { { HostPort = "8088" } } },
        RestartPolicy = { Name = "unless-stopped" },
      },
    }, { name = "web", platform = "linux/amd64" })
    if made == nil then
      assert(create_err ~= nil)
      if create_err.kind == "api" then
        local _status: number? = create_err.status
        local _reason: string = create_err.reason
      elseif create_err.kind == "too_large" then
        local _limit: number = create_err.limit
      elseif create_err.kind == "timed_out" then
        local _message: string = create_err.message
      end
      return
    end
    local _id: string = made.Id
    assert(local_docker:start_container(made.Id))
    for _, container in assert(local_docker:list_containers { all = true, limit = 5, size = true }) do
      local _state: string? = container.State
      local names = container.Names
      if names ~= nil then
        local _first: string = names[1]
      end
    end
    local info = assert(local_docker:info())
    local _server: string? = info.ServerVersion
    local _count: number? = info.Containers
    local _now: string? = info.SystemTime
    local web, inspect_failed = local_docker:inspect_container("web")
    if web ~= nil then
      local _created: string? = web.Created
      local state = web.State
      if state ~= nil then
        local _running: boolean? = state.Running
        local _started: string? = state.StartedAt
      end
      local config = web.Config
      if config ~= nil then
        local _labels: { [string]: string }? = config.Labels
      end
    elseif inspect_failed ~= nil then
      local _kind: string = inspect_failed.kind
    end
    local stats = assert(local_docker:stats("web"))
    local _read: string? = stats.read
    local memory = stats.memory_stats
    if memory ~= nil then
      local _usage: number? = memory.usage
    end
    local result = assert(local_docker:exec("web", { "nginx", "-t" }, { env = { "A=1" }, user = "root" }))
    local _code: number? = result.exit_code
    local _out: string = result.stdout .. result.stderr
    for log in local_docker:logs("web", { follow = true, since = 0, timestamps = true }):items() do
      local _from: "stdout" | "stderr" | "stdin" | "console" = log.stream
      local _data: string = log.data
    end
    local pull = local_docker:pull_image("nginx", { platform = "linux/amd64" })
    local progress, pull_err = pull:next()
    if progress ~= nil then
      local _status: string? = progress.status
      local detail = progress.progressDetail
      if detail ~= nil then
        local _total: number? = detail.total
      end
    elseif pull_err ~= nil then
      local _kind: string = pull_err.kind
    end
    pull:close()
    local happened = local_docker:events({ since = 0, filters = { event = { "start", "die" } } }):next()
    if happened ~= nil then
      local _action: string? = happened.Action
      local actor = happened.Actor
      if actor ~= nil then
        local _actor: string? = actor.ID
      end
    end
    for _, image in assert(local_docker:list_images { digests = true }) do
      local _tags: { string }? = image.RepoTags
      local _size: number = image.Size
    end
    for _, removed in assert(local_docker:remove_image("nginx:1.27", { force = true, noprune = true })) do
      local _untagged: string? = removed.Untagged
    end
    local data = assert(local_docker:create_volume { Name = "data", Driver = "local", Labels = { app = "web" } })
    local _mountpoint: string = data.Mountpoint
    local _made: string? = data.CreatedAt
    local _scope: ("local" | "global")? = data.Scope
    local volumes = assert(local_docker:list_volumes { filters = { label = { "app=web" } } })
    local _warnings: { string } = volumes.Warnings
    for _, volume in volumes.Volumes do
      local _name: string = volume.Name
      local _labels: { [string]: string } = volume.Labels
      local usage = volume.UsageData
      if usage ~= nil then
        local _size: number = usage.Size
      end
    end
    for _, known in assert(local_docker:list_networks { filters = { driver = { "bridge" } } }) do
      local _name: string? = known.Name
      local _since: string? = known.Created
    end
    local network =
      assert(local_docker:create_network { Name = "backend", Driver = "bridge", Labels = { app = "web" } })
    local _network: string = network.Id
    assert(local_docker:remove_network("backend"))
    assert(local_docker:stop_container("web", { timeout = 5, signal = "SIGINT" }))
    assert(local_docker:restart_container("web"))
    assert(local_docker:kill_container("web", { signal = "SIGHUP" }))
    assert(local_docker:pause_container("web"))
    assert(local_docker:unpause_container("web"))
    assert(local_docker:remove_container("web", { force = true, volumes = true }))
    assert(local_docker:remove_volume("data", { force = true }))
  end

  -- the module cannot be patched
  assert(not pcall(function()
    (bollard :: any).connect = nil
  end))
end

return out
