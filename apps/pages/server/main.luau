--!strict
-- The Pages app's backend: websites served from folders of the machine, by a web server this app does not
-- implement. The server is nginx, in a container this app makes in Docker and keeps (sidecar.luau); what this app
-- has of its own is the configuration the container is made from (config.luau): the sites, each a folder with a
-- port of its own, names on a shared port, or both. Every change of the configuration is the container made again,
-- and with no site on there is no container.
--
-- Every call that changes something answers with the overview as it now stands. A change Docker refused (a port is
-- taken, the image cannot be pulled) is not a failed call: the configuration was kept, and the overview says the
-- server is not running and why (`server.problem`). A failed call is one whose input was refused.
--
-- The container changes without the app (it crashes, someone stops it in a terminal): a task watches Docker's events
-- for it for as long as the app is loaded and tells the open windows with the `changed` notification, and they read the
-- overview again.
--
-- An agent can read what is hosted (`list_sites`), and host and stop hosting a folder (`add_site`, `remove_site`).
-- Turning a site on and off, and the settings, are left to the person. It serves on the RPC bus through the global
-- `RPC`, typed by the generated `types/vos.d.luau`; the page's methods are private (`_overview`, ...).
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

local function overview(): pages_Overview
  return {
    engine = docker.status(),
    server = sidecar.server(),
    settings = config.settings(),
    sites = sidecar.sites(),
  }
end

-- The configuration changed: the container is made again from it when that makes a difference, and every window is
-- told (a change that leaves the container alone is none of Docker's events).
local function reconfigured()
  sidecar.apply(false)
  changed()
end

RPC.on_request("_overview", function(): pages_OverviewResult
  return overview()
end)

RPC.on_request("_connect", function(params: pages_ConnectParams, ctx: vos_app_Context): pages_ConnectResult
  local refused = docker.use(params.host)
  return if refused then ctx:fail(refused) else overview()
end)

RPC.on_request("_apply", function(): pages_ApplyResult
  sidecar.apply(true)
  changed()
  return overview()
end)

RPC.on_request(
  "_save_settings",
  function(params: pages_SaveSettingsParams, ctx: vos_app_Context): pages_SaveSettingsResult
    local refused = config.saveSettings(params.port)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end
)

RPC.on_request("_create_site", function(params: pages_CreateSiteParams, ctx: vos_app_Context): pages_CreateSiteResult
  local refused = config.add(params)
  if refused then
    return ctx:fail(refused)
  end
  reconfigured()
  return overview()
end)

RPC.on_request("_update_site", function(params: pages_UpdateSiteParams, ctx: vos_app_Context): pages_UpdateSiteResult
  local refused = config.update(params.site, params)
  if refused then
    return ctx:fail(refused)
  end
  reconfigured()
  return overview()
end)

RPC.on_request(
  "_set_site_enabled",
  function(params: pages_SetSiteEnabledParams, ctx: vos_app_Context): pages_SetSiteEnabledResult
    local refused = config.setEnabled(params.site, params.enabled)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end
)

RPC.on_request("_delete_site", function(params: pages_DeleteSiteParams, ctx: vos_app_Context): pages_DeleteSiteResult
  local gone = config.remove(function(site)
    return site.id == params.site
  end)
  if gone == 0 then
    return ctx:fail(config.NO_SITE)
  end
  reconfigured()
  return overview()
end)

RPC.on_request("_logs", function(params: pages_LogsParams, ctx: vos_app_Context): pages_LogsResult
  local logs, refused = sidecar.logs(params.tail)
  return logs or ctx:fail(refused or "The server has no container.")
end)

-- how the server is doing, as an agent reads it
local function serverOf(): pages_Brief
  local server = sidecar.server()
  return { state = server.state, sharedPort = config.settings().port, problem = server.problem }
end

RPC.on_request("list_sites", function(): pages_ListSitesResult
  local sites: { pages_Hosted } = {}
  for _, site in sidecar.sites() do
    table.insert(sites, {
      name = site.name,
      folder = site.folder,
      port = site.port,
      hostnames = site.hostnames,
      enabled = site.enabled,
      problem = site.problem,
    })
  end
  return { server = serverOf(), sites = sites }
end)

RPC.on_request("add_site", function(params: pages_AddSiteParams, ctx: vos_app_Context): pages_AddSiteResult
  local refused = config.add {
    name = params.name,
    folder = params.folder,
    port = params.port or 0,
    hostnames = params.hostnames or {},
  }
  if refused then
    return ctx:fail(refused)
  end
  reconfigured()
  return { added = true, server = serverOf() }
end)

RPC.on_request("remove_site", function(params: pages_RemoveSiteParams, ctx: vos_app_Context): pages_RemoveSiteResult
  local removed = config.remove(function(site)
    return site.name == params.name
  end)
  if removed == 0 then
    return ctx:fail(config.NO_SITE)
  end
  reconfigured()
  return { removed = removed, server = serverOf() }
end)
