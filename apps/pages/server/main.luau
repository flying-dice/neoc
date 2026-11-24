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
-- for it for as long as the app is loaded and tells the open windows with a `changed` event, and they read the
-- overview again.
--
-- An agent can read what is hosted (`list_sites`), and host and stop hosting a folder (`add_site`, `remove_site`).
-- Turning a site on and off, and the settings, are left to the person.
local app = require("vos:app")
local config = require("./config")
local docker = require("vos:docker")
local rpc = require("../.app/rpc")
local sidecar = require("./sidecar")
local task = require("std:task")
local t = app.t

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
    app.emit("changed")
  end)
end

-- the watcher is the app's own task: it starts at load without holding it up, and stops when the app is unloaded
task.spawn(docker.watch, sidecar.FILTERS, changed)

local function overview(): rpc.Overview
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

local handlers: rpc.Handlers = {
  overview = function(_ctx)
    return overview()
  end,

  connect = function(ctx, params)
    local refused = docker.use(params.host)
    return if refused then ctx:fail(refused) else overview()
  end,

  apply = function(_ctx)
    sidecar.apply(true)
    changed()
    return overview()
  end,

  saveSettings = function(ctx, params)
    local refused = config.saveSettings(params.port)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end,

  addSite = function(ctx, params)
    local refused = config.add(params)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end,

  updateSite = function(ctx, params)
    local refused = config.update(params.site, params)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end,

  setSiteEnabled = function(ctx, params)
    local refused = config.setEnabled(params.site, params.enabled)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end,

  removeSite = function(ctx, params)
    local gone = config.remove(function(site)
      return site.id == params.site
    end)
    if gone == 0 then
      return ctx:fail(config.NO_SITE)
    end
    reconfigured()
    return overview()
  end,

  logs = function(ctx, params)
    local logs, refused = sidecar.logs(params.tail)
    return logs or ctx:fail(refused or "The server has no container.")
  end,
}

-- how the server is doing, as an agent reads it
local function serverOf(): { state: rpc.ServerState, sharedPort: number, problem: string }
  local server = sidecar.server()
  return { state = server.state, sharedPort = config.settings().port, problem = server.problem }
end

app.tool {
  name = "list_sites",
  title = "List the hosted sites",
  readOnly = true,
  description = "The websites this machine hosts, each a folder of files: its name, the folder, its own port (0 when it has none), the hostnames it answers to on the shared port, whether it is on (`enabled`), and why it is not served when its folder is missing (`problem`). Also how the web server is doing: the state of its container (`running`, `stopped`, `restarting`, or `absent` when there is none, as with every site off), the port the sites with hostnames share (`sharedPort`), and why it is not running when Docker refused (`problem`).",
  input = {},
  run = function(_input, _ctx)
    local sites: { { name: string, folder: string, port: number, hostnames: { string }, enabled: boolean, problem: string } } =
      {}
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
  end,
}

app.tool {
  name = "add_site",
  title = "Host a site",
  description = "Serve a folder of this machine as a website: its `index.html` is the front page, and its files are only read. Give it a port of its own (`http://<machine>:8081`), hostnames it answers to on the shared port (`http://blog.home`, which must resolve to this machine), or both. It is served a moment later.",
  input = {
    name = t.string { description = "What the site is called in the list", maxLength = 80 },
    folder = t.string { description = "The full path of the folder, like `/Users/me/Sites/blog`", maxLength = 1024 },
    port = t.integer {
      optional = true,
      description = "A port of its own, like 8081; none when left out",
      min = 1,
      max = 65535,
    },
    hostnames = t.array(t.string { maxLength = 253 }, {
      optional = true,
      description = "Names it answers to on the shared port, like `blog.home`; none when left out",
    }),
  },
  run = function(input: { name: string, folder: string, port: number?, hostnames: { string }? }, ctx)
    local refused = config.add {
      name = input.name,
      folder = input.folder,
      port = input.port or 0,
      hostnames = input.hostnames or {},
    }
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return { added = true, server = serverOf() }
  end,
}

app.tool {
  name = "remove_site",
  title = "Stop hosting a site",
  description = "Stop serving a site and forget it, by its name as `list_sites` gives it. Its folder and the files in it are not touched.",
  input = {
    name = t.string { description = "The name of the site", maxLength = 80 },
  },
  run = function(input: { name: string }, ctx)
    local removed = config.remove(function(site)
      return site.name == input.name
    end)
    if removed == 0 then
      return ctx:fail(config.NO_SITE)
    end
    reconfigured()
    return { removed = removed, server = serverOf() }
  end,
}

return handlers
