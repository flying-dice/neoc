--!strict
-- The DNS app's backend: a DNS server for the person's network, which this app does not implement. The server is
-- dnsmasq, in a container this app makes in Docker and keeps (sidecar.luau); what this app has of its own is the
-- configuration the container is made from (config.luau): whether the server is wanted on, its settings, and the
-- records. Every change of the configuration is the container made again, when the server is on.
--
-- Every call that changes something answers with the overview as it now stands. A change Docker refused (the port
-- is taken, the image cannot be pulled) is not a failed call: the configuration was kept, and the overview says the
-- server is not running and why (`server.problem`). A failed call is one whose input was refused.
--
-- The container changes without the app (it crashes, someone stops it in a terminal): a task watches Docker's events
-- for it for as long as the app is loaded and tells the open windows with the `changed` notification, and they read the
-- overview again.
--
-- An agent can read what the server answers with (`list_records`), and give and take names (`add_record`,
-- `remove_record`). Turning the server on and off, and its settings, are left to the person. It serves on the RPC bus
-- through the global `RPC`, typed by the generated `types/vos.d.luau`; the page's methods are private (`_overview`, ...).
local config = require("./config")
local docker = require("vos:docker")
local sidecar = require("./sidecar")
local task = require("std:task")

-- how long the windows are left alone after being told of a change, in seconds: making the container again is
-- several of Docker's events
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
task.spawn(docker.watch, sidecar.FILTERS, changed)

local function overview(): dns_Overview
  return {
    engine = docker.status(),
    server = sidecar.server(),
    settings = config.settings(),
    records = config.records(),
  }
end

-- The configuration changed: a server that is on is made again from it, and every window is told (with the server
-- off Docker has nothing to report).
local function reconfigured()
  if config.enabled() then
    sidecar.apply()
  end
  changed()
end

RPC.on_request("_overview", function(): dns_OverviewResult
  return overview()
end)

RPC.on_request("_connect", function(params: dns_ConnectParams, ctx: vos_app_Context): dns_ConnectResult
  local refused = docker.use(params.host)
  return if refused then ctx:fail(refused) else overview()
end)

RPC.on_request("_set_enabled", function(params: dns_SetEnabledParams): dns_SetEnabledResult
  config.setEnabled(params.enabled)
  if params.enabled then
    sidecar.apply()
  else
    sidecar.remove()
  end
  changed()
  return overview()
end)

RPC.on_request("_apply", function(): dns_ApplyResult
  reconfigured()
  return overview()
end)

RPC.on_request("_save_settings", function(params: dns_SaveSettingsParams, ctx: vos_app_Context): dns_SaveSettingsResult
  local refused = config.saveSettings(params.port, params.upstreams, params.logQueries)
  if refused then
    return ctx:fail(refused)
  end
  reconfigured()
  return overview()
end)

RPC.on_request("_create_record", function(params: dns_CreateRecordParams, ctx: vos_app_Context): dns_CreateRecordResult
  local refused = config.add(params.name, params.address, params.subdomains)
  if refused then
    return ctx:fail(refused)
  end
  reconfigured()
  return overview()
end)

RPC.on_request("_delete_record", function(params: dns_DeleteRecordParams, ctx: vos_app_Context): dns_DeleteRecordResult
  local gone = config.remove(function(record)
    return record.id == params.record
  end)
  if gone == 0 then
    return ctx:fail("There is no such record.")
  end
  reconfigured()
  return overview()
end)

RPC.on_request("_logs", function(params: dns_LogsParams, ctx: vos_app_Context): dns_LogsResult
  local logs, refused = sidecar.logs(params.tail)
  return logs or ctx:fail(refused or "The server has no container.")
end)

-- how the server is doing, as an agent reads it
local function serverOf(): { enabled: boolean, state: dns_ServerState, port: number, problem: string }
  local server = sidecar.server()
  return { enabled = server.enabled, state = server.state, port = config.settings().port, problem = server.problem }
end

RPC.on_request("list_records", function(): dns_ListRecordsResult
  local records: { { name: string, address: string, subdomains: boolean } } = {}
  for _, record in config.records() do
    table.insert(records, { name = record.name, address = record.address, subdomains = record.subdomains })
  end
  return { server = serverOf(), upstreams = config.settings().upstreams, records = records }
end)

RPC.on_request("add_record", function(params: dns_AddRecordParams, ctx: vos_app_Context): dns_AddRecordResult
  local refused = config.add(params.name, params.address, params.subdomains == true)
  if refused then
    return ctx:fail(refused)
  end
  reconfigured()
  return { added = true, server = serverOf() }
end)

RPC.on_request("remove_record", function(params: dns_RemoveRecordParams, ctx: vos_app_Context): dns_RemoveRecordResult
  local name = string.lower(params.name)
  local address = if params.address then string.lower(params.address) else nil
  local removed = config.remove(function(record)
    return record.name == name and (address == nil or record.address == address)
  end)
  if removed == 0 then
    return ctx:fail("There is no such record.")
  end
  reconfigured()
  return { removed = removed, server = serverOf() }
end)
