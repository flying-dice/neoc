--!strict
-- The Services app's backend: the background services of the machine, as its own service manager has them. It keeps
-- nothing of its own (manager.luau asks the operating system every time): a service another program registered,
-- started or removed shows at the next overview. Every call that changes something answers with an outcome and the
-- overview as it now stands, so the window that asked draws what the machine has.
--
-- An agent can see which services there are and how they are doing (`list_services`). Changing a service needs the
-- rights of the host and the person's say, so starting, stopping, registering and removing are left to the person.
--
-- It serves on the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`. The page's methods are
-- private (`_overview`, `_start`, ...); `list_services` is the public one, the agents' tool.
local manager = require("./manager")

RPC.on_request("_overview", function(): services_OverviewResult
  return manager.overview()
end)

RPC.on_request("_start", function(params: services_StartParams): services_StartResult
  return manager.control("start", params.label)
end)

RPC.on_request("_stop", function(params: services_StopParams): services_StopResult
  return manager.control("stop", params.label)
end)

RPC.on_request("_restart", function(params: services_RestartParams): services_RestartResult
  return manager.control("restart", params.label)
end)

RPC.on_request("_configure", function(params: services_ConfigureParams): services_ConfigureResult
  return manager.configure(
    params.label,
    { autostart = params.autostart, description = params.description, restart = params.restart }
  )
end)

RPC.on_request("_register", function(params: services_RegisterParams): services_RegisterResult
  local environment: { [string]: string }? = nil
  if params.environment ~= nil then
    local map: { [string]: string } = {}
    for _, variable in params.environment do
      map[variable.name] = variable.value
    end
    environment = map
  end
  return manager.register {
    label = params.label,
    program = params.program,
    args = params.args,
    description = params.description,
    working_directory = params.workingDirectory,
    environment = environment,
    autostart = params.autostart,
    restart = params.restart,
    restart_delay_secs = params.restartDelaySecs,
  }
end)

RPC.on_request("_unregister", function(params: services_UnregisterParams): services_UnregisterResult
  return manager.unregister(params.label)
end)

RPC.on_request(
  "list_services",
  function(params: services_ListServicesParams, ctx: vos_app_Context): services_ListServicesResult
    local overview = manager.overview()
    if not overview.manager.available then
      return ctx:fail(overview.manager.problem)
    end
    local services: { services_Listed } = {}
    for _, service in overview.services do
      if params.state == nil or service.state == params.state then
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
  end
)
