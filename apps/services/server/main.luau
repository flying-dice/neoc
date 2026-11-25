--!strict
-- The Services app's backend: the background services of the machine, as its own service manager has them. It keeps
-- nothing of its own (manager.luau asks the operating system every time): a service another program registered,
-- started or removed shows at the next overview. Every call that changes something answers with an outcome and the
-- overview as it now stands, so the window that asked draws what the machine has.
--
-- An agent can see which services there are and how they are doing (`list_services`). Changing a service needs the
-- rights of the host and the person's say, so starting, stopping, registering and removing are left to the person.
local app = require("vos:app")
local manager = require("./manager")
local rpc = require("../.app/rpc")
local t = app.t

local handlers: rpc.Handlers = {
  overview = function(_ctx)
    return manager.overview()
  end,

  start = function(_ctx, params)
    return manager.control("start", params.label)
  end,

  stop = function(_ctx, params)
    return manager.control("stop", params.label)
  end,

  restart = function(_ctx, params)
    return manager.control("restart", params.label)
  end,

  configure = function(_ctx, params)
    return manager.configure(
      params.label,
      { autostart = params.autostart, description = params.description, restart = params.restart }
    )
  end,

  register = function(_ctx, params)
    return manager.register {
      label = params.label,
      program = params.program,
      args = params.args,
      description = params.description,
      working_directory = params.workingDirectory,
      autostart = params.autostart,
      restart = params.restart,
    }
  end,

  unregister = function(_ctx, params)
    return manager.unregister(params.label)
  end,
}

type Listed = { label: string, description: string?, state: rpc.State, autostart: boolean?, program: string? }

app.tool {
  name = "list_services",
  title = "List the services",
  readOnly = true,
  description = "The background services of this machine, as its service manager (systemd, launchd or the Windows Service Control Manager) has them, whoever registered them: the label, what it is for, whether it is running, whether it starts by itself and the program it runs. Give `state` for the services in one state only.",
  input = {
    state = t.string {
      optional = true,
      description = "running, stopped, failed, starting, stopping, paused or unknown; every service when left out",
    },
  },
  run = function(input: { state: string? }, ctx)
    local overview = manager.overview()
    if not overview.manager.available then
      return ctx:fail(overview.manager.problem)
    end
    local services: { Listed } = {}
    for _, service in overview.services do
      if input.state == nil or service.state == input.state then
        table.insert(services, {
          label = service.label,
          description = service.description,
          state = service.state,
          autostart = service.autostart,
          program = service.program,
        })
      end
    end
    return { manager = overview.manager.kind, services = services }
  end,
}

return handlers
