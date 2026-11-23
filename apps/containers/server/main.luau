--!strict
-- The Containers app's backend: the Docker containers and Compose stacks of a machine. It keeps nothing of its own
-- (the choice of which Docker is kept by `vos:docker`): the containers are Docker's, read each time they are asked for.
-- Every call that changes something answers with the overview as it now stands, so the window that asked draws what
-- Docker has.
--
-- Docker changes without the app (a container exits, someone runs `docker compose up` in a terminal): a task watches
-- Docker's events for as long as the app is loaded and tells the open windows with a `changed` event, and they read
-- the overview again.
--
-- An agent can do what a person does most: see what runs (`list_containers`), read a container's logs
-- (`container_logs`), and start, stop and restart a container or a whole stack. Removing is left to the person. A tool
-- that changed something tells the windows itself, so they follow even while the watcher is between two Dockers.
local app = require("vos:app")
local engine = require("./engine")
local rpc = require("../.app/rpc")
local task = require("std:task")
local t = app.t

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
    app.emit("changed")
  end)
end

-- the watcher is the app's own task: it starts at load without holding it up, and stops when the app is unloaded
task.spawn(engine.watch, changed)

local handlers: rpc.Handlers = {
  overview = function(_ctx)
    return engine.overview()
  end,

  connect = function(ctx, params)
    local problem = engine.use(params.host)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  start = function(ctx, params)
    local problem = engine.container("start", params.container)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  stop = function(ctx, params)
    local problem = engine.container("stop", params.container)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  restart = function(ctx, params)
    local problem = engine.container("restart", params.container)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  remove = function(ctx, params)
    local problem = engine.container("remove", params.container)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  startStack = function(ctx, params)
    local problem = engine.stack("start", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  stopStack = function(ctx, params)
    local problem = engine.stack("stop", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  restartStack = function(ctx, params)
    local problem = engine.stack("restart", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  removeStack = function(ctx, params)
    local problem = engine.stack("remove", params.stack)
    return if problem then ctx:fail(problem) else engine.overview()
  end,

  details = function(ctx, params)
    local details, problem = engine.details(params.container)
    return details or ctx:fail(problem or "That container is not there any more.")
  end,

  logs = function(ctx, params)
    local logs, problem = engine.logs(params.container, params.tail)
    return logs or ctx:fail(problem or "That container is not there any more.")
  end,
}

-- a container as an agent reads it
type Listed = { name: string, image: string, state: rpc.State, status: string, ports: { string }, stack: string }

app.tool {
  name = "list_containers",
  title = "List the containers",
  readOnly = true,
  description = "Every Docker container of this machine, running or not: its name, image, state (running, exited, paused, ...), Docker's own words for it (`Up 2 hours`), its published ports (`8080:80`) and the Compose stack it belongs to (empty for a container on its own). Give `stack` for the containers of one stack only.",
  input = {
    stack = t.string { optional = true, description = "The name of a Compose stack; every container when left out" },
  },
  run = function(input: { stack: string? }, ctx)
    local overview = engine.overview()
    if not overview.engine.reachable then
      return ctx:fail(overview.engine.problem)
    end
    local containers: { Listed } = {}
    for _, container in overview.containers do
      if input.stack == nil or container.stack == input.stack then
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
  end,
}

app.tool {
  name = "container_logs",
  title = "Read a container's logs",
  readOnly = true,
  description = "The end of what a container has written (its output and its errors together), by the container's name (`list_containers` gives the names).",
  input = {
    container = t.string { description = "The name of the container", maxLength = 128 },
    lines = t.integer {
      optional = true,
      description = "How many lines from the end; 200 when left out",
      min = 1,
      max = 2000,
    },
  },
  run = function(input: { container: string, lines: number? }, ctx)
    local logs, problem = engine.logs(input.container, input.lines)
    if not logs then
      return ctx:fail(problem or "There is no such container.")
    end
    return { container = input.container, logs = logs.text, truncated = logs.truncated }
  end,
}

-- the tools that act on one container, and on one stack: each answers with what it did
local CONTAINER_TOOLS: { { action: engine.Action, title: string, done: string, description: string } } = {
  {
    action = "start",
    title = "Start a container",
    done = "started",
    description = "Start a stopped container, by its name.",
  },
  {
    action = "stop",
    title = "Stop a container",
    done = "stopped",
    description = "Stop a running container, by its name: Docker asks it to end and kills it after ten seconds.",
  },
  {
    action = "restart",
    title = "Restart a container",
    done = "restarted",
    description = "Stop a container and start it again, by its name.",
  },
}

for _, tool in CONTAINER_TOOLS do
  app.tool {
    name = `{tool.action}_container`,
    title = tool.title,
    description = tool.description,
    input = { container = t.string { description = "The name of the container", maxLength = 128 } },
    run = function(input: { container: string }, ctx)
      local problem = engine.container(tool.action, input.container)
      if problem then
        return ctx:fail(problem)
      end
      changed()
      return { container = input.container, [tool.done] = true }
    end,
  }
end

local STACK_TOOLS: { { action: engine.Action, title: string, done: string, description: string } } = {
  {
    action = "start",
    title = "Start a stack",
    done = "started",
    description = "Start every container of a Compose stack, by the stack's name (`list_containers` gives each container's stack). The services others wait for start first.",
  },
  {
    action = "stop",
    title = "Stop a stack",
    done = "stopped",
    description = "Stop every container of a Compose stack, by the stack's name. Nothing is removed.",
  },
}

for _, tool in STACK_TOOLS do
  app.tool {
    name = `{tool.action}_stack`,
    title = tool.title,
    description = tool.description,
    input = { stack = t.string { description = "The name of the stack", maxLength = 128 } },
    run = function(input: { stack: string }, ctx)
      local problem = engine.stack(tool.action, input.stack)
      if problem then
        return ctx:fail(problem)
      end
      changed()
      return { stack = input.stack, [tool.done] = true }
    end,
  }
end

return handlers
