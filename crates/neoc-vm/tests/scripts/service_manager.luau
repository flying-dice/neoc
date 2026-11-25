--!strict
-- vnd:service_manager as a script sees it: its answers (service_manager.json), then what it asserts. Only mock
-- managers: nothing here touches the services of whoever runs it (`service_manager.manager()` is not called).
local env = require("std:env")
local sm = require("vnd:service_manager")
local out = {}

-- services other programs registered are listed and managed like one made here
local manager = sm.mock({
  { label = "nginx", description = "A web server", state = "running", autostart = true, program = "/usr/sbin/nginx", args = { "-g", "daemon off;" }, restart = "always", restart_delay_secs = 2 },
  { label = "locked", state = "running", locked = true },
  { label = "broken", broken = true },
})
local listed = assert(manager:list())
local labels = {}
for _, service in listed do
  table.insert(labels, service.label)
end
out.labels = labels

local nginx = assert(manager:get("nginx"))
out.nginx = { nginx.state, nginx.autostart, nginx.program, nginx.args, nginx.restart, nginx.restart_delay_secs }
local absent, absent_err = manager:get("absent")
out.absent = { absent == nil, absent_err == nil }

-- configure changes what is given and keeps the rest
local configured = assert(manager:configure("nginx", { autostart = false }))
out.configured = { configured.autostart, configured.description, configured.program, configured.restart, configured.restart_delay_secs }
out.stopped = assert(manager:stop("nginx")).state
out.restarted = assert(manager:restart("nginx")).state

-- the capabilities tell which options a manager keeps
local capabilities = manager:capabilities()
out.kind = capabilities.kind
out.level = capabilities.level
out.install_options = capabilities.install_options

-- a service of the machine that this host may not change, and one that cannot start
local _, denied = manager:stop("locked")
out.denied = denied and denied.kind
local _, failed = manager:start("broken")
out.failed = failed and failed.kind
local _, missing = manager:start("absent")
out.missing = missing and missing.kind

-- the life of a service: install, start, inspect, stop, uninstall
-- a program that is there on every platform
local program = if env.family == "windows" then "C:\\Windows\\System32\\cmd.exe" else "/bin/sh"
local installed = assert(manager:install({ label = "app", program = program, args = { "-c", "sleep 1" }, autostart = true, description = "Mine" }))
out.installed = { installed.state, installed.autostart, installed.description }
local _, exists = manager:install({ label = "app", program = program })
out.exists = exists and exists.kind
out.started = assert(manager:start("app")).state
assert(manager:uninstall("app"))
out.uninstalled = manager:get("app") == nil

-- what it asserts
do
  -- an option the manager does not keep is refused by name
  local _, unsupported = manager:install({ label = "other", program = program, user = "daemon" })
  assert(unsupported and unsupported.kind == "unsupported" and unsupported.option == "user")

  -- what cannot be a service is invalid input, with the typed error and its sentence
  local _, invalid = manager:install({ label = "a b", program = program })
  assert(invalid and invalid.kind == "invalid_input")
  assert(
    invalid.message:find("sm.install", 1, true) == nil and tostring(invalid.op) == "vnd:service_manager.Manager:install"
  )
  local _, relative = manager:install({ label = "x", program = "relative/program" })
  assert(relative and relative.kind == "invalid_input")
  local _, injected = manager:install({ label = "x", program = program, description = "a\nExecStart=/bin/evil" })
  assert(injected and injected.kind == "invalid_input")

  -- an unknown option never gets here
  local ok, refused = pcall(function()
    return (manager :: any):install({ label = "y", program = program, nonsense = true })
  end)
  assert(not ok and tostring(refused):find("nonsense", 1, true))
end

return out
