--!strict
-- The service manager of the machine the host runs on, as this app uses it: the one place that talks to the operating
-- system (through `vnd:service_manager`) and says in the app's own words what went wrong. That is systemd on Linux,
-- launchd on macOS and the Service Control Manager on Windows.
--
-- The operating system is the source of truth: the app keeps nothing of its own. Every overview is asked of the
-- manager, so a service another program registered, started or removed is seen at the next call.
--
-- The app never raises its own rights. A change the operating system refuses for want of rights is `denied`, said in
-- a sentence. A change made of several steps may have done some of them before the refusal, so what is said is what
-- the machine has now (the overview that comes with every outcome), not that nothing happened.
--
-- A function that changes something answers with an outcome: a status, a sentence for what the person needs to know,
-- and the overview as it now stands, so the window that asked draws what the machine has.
local sm = require("vnd:service_manager")
local tracing = require("lib:tracing")

local manager = {}

-- Why a call failed, as a sentence for the person, and which of the app's statuses it is.
local function sentence(err: vnd_service_manager_Error): (services_Status, string)
  if err.kind == "permission_denied" then
    return "denied",
      "The operating system did not allow this. Changing a service needs administrator rights, which this app does not raise on its own."
  elseif err.kind == "unsupported" then
    return "unsupported", "This machine's service manager cannot do that: " .. err.reason
  elseif err.kind == "invalid_input" then
    return "invalid", err.reason
  elseif err.kind == "not_found" then
    return "missing", "This service is not there any more."
  elseif err.kind == "already_exists" then
    return "invalid", "A service called " .. err.label .. " is there already."
  elseif err.kind == "unavailable" then
    return "failed", "This machine has no service manager the app can use: " .. err.reason
  elseif err.kind == "failed" then
    return "failed", "The service manager could not " .. err.operation .. ": " .. err.reason
  end
  return "failed", if err.message ~= "" then err.message else "The service manager refused, and did not say why."
end

local function variables(environment: { [string]: string }?): { services_Variable }?
  if environment == nil then
    return nil
  end
  local listed: { services_Variable } = {}
  for name, value in environment do
    table.insert(listed, { name = name, value = value })
  end
  table.sort(listed, function(a, b)
    return a.name < b.name
  end)
  return listed
end

local function serviceOf(service: vnd_service_manager_Service): services_Service
  return {
    label = service.label,
    description = service.description,
    state = service.state,
    stateDetail = service.state_detail,
    autostart = service.autostart,
    program = service.program,
    args = service.args,
    commandLine = service.command_line,
    workingDirectory = service.working_directory,
    environment = variables(service.environment),
    restart = service.restart,
    restartDelaySecs = service.restart_delay_secs,
    user = service.user,
    path = service.path,
  }
end

local function open(): (vnd_service_manager_Manager?, string?)
  local opened, err = sm.manager()
  if err then
    local _, problem = sentence(err)
    return nil, problem
  end
  return opened, nil
end

local function describe(opened: vnd_service_manager_Manager): services_Manager
  local capabilities = opened:capabilities()
  return {
    available = true,
    problem = "",
    kind = capabilities.kind,
    level = capabilities.level,
    installOptions = capabilities.install_options,
    configurable = capabilities.configurable,
    reported = capabilities.reported,
  }
end

local function unavailable(problem: string): services_Overview
  return {
    manager = {
      available = false,
      problem = problem,
      kind = "",
      level = "system",
      installOptions = {},
      configurable = {},
      reported = {},
    },
    services = {},
  }
end

-- every service the manager has, with what the manager can do; a manager that cannot list is an answer too
function manager.overview(): services_Overview
  local opened, problem = open()
  if not opened then
    return unavailable(problem or "")
  end
  local listed, err = opened:list()
  if err then
    local _, why = sentence(err)
    return unavailable(why)
  end
  local services: { services_Service } = {}
  for _, service in assert(listed) do
    table.insert(services, serviceOf(service))
  end
  return { manager = describe(opened), services = services }
end

local function outcome(status: services_Status, message: string): services_Outcome
  return { status = status, message = message, overview = manager.overview() }
end

local function failure(err: vnd_service_manager_Error, label: string): services_Outcome
  local status, message = sentence(err)
  tracing.warn("the service manager refused a change", { error = err.message, kind = err.kind, label = label })
  return outcome(status :: services_Status, message)
end

local function opened(): (vnd_service_manager_Manager?, services_Outcome?)
  local found, problem = open()
  if not found then
    tracing.warn("could not open the service manager", { error = problem })
    return nil, outcome("failed", problem or "")
  end
  return found, nil
end

-- start, stop or restart a service
function manager.control(action: "start" | "stop" | "restart", label: string): services_Outcome
  local found, refused = opened()
  if not found then
    return assert(refused)
  end
  local _, err
  if action == "start" then
    _, err = found:start(label)
  elseif action == "stop" then
    _, err = found:stop(label)
  else
    _, err = found:restart(label)
  end
  return if err then failure(err, label) else outcome("done", "")
end

function manager.configure(
  label: string,
  change: { autostart: boolean?, description: string?, restart: services_Restart? }
): services_Outcome
  local found, refused = opened()
  if not found then
    return assert(refused)
  end
  local _, err = found:configure(label, {
    autostart = change.autostart,
    description = change.description,
    program = nil,
    args = nil,
    working_directory = nil,
    environment = nil,
    restart = change.restart,
    restart_delay_secs = nil,
  })
  if err then
    local status, message = sentence(err)
    tracing.warn("the service manager refused a change", { error = err.message, kind = err.kind, label = label })
    -- the settings are written one after the other: a refusal or a failure may come after the first was made (what is
    -- invalid or unsupported is refused before anything is written)
    if status == "denied" or status == "failed" then
      message ..= " Part of the change may already have been made; the details show what the machine has now."
    end
    return outcome(status :: services_Status, message)
  end
  return outcome("done", "")
end

function manager.register(spec: vnd_service_manager_Spec): services_Outcome
  local found, refused = opened()
  if not found then
    return assert(refused)
  end
  local _, err = found:install(spec)
  return if err then failure(err, spec.label or "") else outcome("done", "")
end

function manager.unregister(label: string): services_Outcome
  local found, refused = opened()
  if not found then
    return assert(refused)
  end
  local _, err = found:uninstall(label)
  return if err then failure(err, label) else outcome("done", "")
end

return manager
