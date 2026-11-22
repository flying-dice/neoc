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
-- for it for as long as the app is loaded and tells the open windows with a `changed` event, and they read the
-- overview again.
--
-- An agent can read what the server answers with (`list_records`), and give and take names (`add_record`,
-- `remove_record`). Turning the server on and off, and its settings, are left to the person.
local app = require("vos:app")
local config = require("./config")
local engine = require("./engine")
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
task.spawn(engine.watch, { type = "container", label = sidecar.FILTER.label, event = sidecar.EVENTS }, changed)

local function overview(): rpc.Overview
  local version, unreachable = engine.version()
  return {
    engine = {
      reachable = version ~= nil,
      version = version or "",
      host = engine.host(),
      problem = if version then "" else unreachable or "",
    },
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

local handlers: rpc.Handlers = {
  overview = function(_ctx)
    return overview()
  end,

  connect = function(ctx, params)
    local refused = engine.use(params.host)
    return if refused then ctx:fail(refused) else overview()
  end,

  setEnabled = function(_ctx, params)
    config.setEnabled(params.enabled)
    if params.enabled then
      sidecar.apply()
    else
      sidecar.remove()
    end
    changed()
    return overview()
  end,

  apply = function(_ctx)
    reconfigured()
    return overview()
  end,

  saveSettings = function(ctx, params)
    local refused = config.saveSettings(params.port, params.upstreams, params.logQueries)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end,

  addRecord = function(ctx, params)
    local refused = config.add(params.name, params.address, params.subdomains)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return overview()
  end,

  removeRecord = function(ctx, params)
    local gone = config.remove(function(record)
      return record.id == params.record
    end)
    if gone == 0 then
      return ctx:fail("There is no such record.")
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
local function serverOf(): { enabled: boolean, state: rpc.ServerState, port: number, problem: string }
  local server = sidecar.server()
  return { enabled = server.enabled, state = server.state, port = config.settings().port, problem = server.problem }
end

app.tool {
  name = "list_records",
  title = "List the DNS records",
  readOnly = true,
  description = "The names this machine's DNS server answers for, each with its address (IPv4 or IPv6) and whether every name under it answers with the address too; the servers it asks for every other name (`upstreams`); and how the server is doing: whether it is wanted on (`enabled`), the state of its container (`running`, `stopped`, `restarting`, or `absent` when there is none), the port it answers on, and why it is not running when Docker refused (`problem`).",
  input = {},
  run = function(_input, _ctx)
    local records: { { name: string, address: string, subdomains: boolean } } = {}
    for _, record in config.records() do
      table.insert(records, { name = record.name, address = record.address, subdomains = record.subdomains })
    end
    return { server = serverOf(), upstreams = config.settings().upstreams, records = records }
  end,
}

app.tool {
  name = "add_record",
  title = "Add a DNS record",
  description = "Give a name an address on this machine's DNS server (`nas.home` -> `192.168.1.10`). A name may have several addresses: add each. A server that is on answers with it a moment later; one that is off keeps it for when it is turned on.",
  input = {
    name = t.string { description = "The name, like `nas.home`", maxLength = 253 },
    address = t.string { description = "An IPv4 or IPv6 address", maxLength = 45 },
    subdomains = t.boolean {
      optional = true,
      description = "Every name under it answers with the address too; false when left out",
    },
  },
  run = function(input: { name: string, address: string, subdomains: boolean? }, ctx)
    local refused = config.add(input.name, input.address, input.subdomains == true)
    if refused then
      return ctx:fail(refused)
    end
    reconfigured()
    return { added = true, server = serverOf() }
  end,
}

app.tool {
  name = "remove_record",
  title = "Remove a DNS record",
  description = "Take a name away from this machine's DNS server, by the name as `list_records` gives it: every address it has, or only `address` when given.",
  input = {
    name = t.string { description = "The name of the record", maxLength = 253 },
    address = t.string {
      optional = true,
      description = "One of the name's addresses; all of them when left out",
      maxLength = 45,
    },
  },
  run = function(input: { name: string, address: string? }, ctx)
    local name = string.lower(input.name)
    local address = if input.address then string.lower(input.address) else nil
    local removed = config.remove(function(record)
      return record.name == name and (address == nil or record.address == address)
    end)
    if removed == 0 then
      return ctx:fail("There is no such record.")
    end
    reconfigured()
    return { removed = removed, server = serverOf() }
  end,
}

return handlers
