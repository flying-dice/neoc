--!strict
-- What the server is made from, kept by the app: whether it is wanted on, its settings, and the records. This is
-- the only copy (the container is made from it, never read back), so it is there with Docker away, and it is the
-- same server in whichever Docker it is started.
local app = require("vos:app")
local dnsmasq = require("./dnsmasq")
local rpc = require("../.app/rpc")

local config = {}

local ENABLED = "enabled"
local SETTINGS = "settings"
local RECORDS = "records"
-- the id the next record gets
local NEXT = "next"

local MAX_RECORDS = 500
local MAX_UPSTREAMS = 8
local DEFAULTS: rpc.Settings = { port = dnsmasq.PORT, upstreams = { "1.1.1.1", "1.0.0.1" }, logQueries = false }

function config.enabled(): boolean
  return app.kv:get(ENABLED) == true
end

function config.setEnabled(enabled: boolean)
  app.kv:set(ENABLED, enabled)
end

function config.settings(): rpc.Settings
  local kept = app.kv:get(SETTINGS)
  if type(kept) ~= "table" then
    return table.clone(DEFAULTS)
  end
  local upstreams: { string } = {}
  for _, upstream in if type(kept.upstreams) == "table" then kept.upstreams else {} do
    table.insert(upstreams, tostring(upstream))
  end
  return {
    port = if type(kept.port) == "number" then kept.port else DEFAULTS.port,
    upstreams = upstreams,
    logQueries = kept.logQueries == true,
  }
end

-- Keep the settings, the upstream servers each once and in the order given. Answers with what is wrong with them
-- when something is, and keeps nothing then.
function config.saveSettings(port: number, upstreams: { string }, logQueries: boolean): string?
  local servers: { string } = {}
  for _, raw in upstreams do
    if string.match(raw, "%S") then
      local server, problem = dnsmasq.address(raw)
      if not server then
        return problem
      end
      if not table.find(servers, server) then
        table.insert(servers, server)
      end
    end
  end
  if #servers > MAX_UPSTREAMS then
    return `There can be {MAX_UPSTREAMS} upstream servers at most.`
  end
  app.kv:set(SETTINGS, { port = port, upstreams = servers, logQueries = logQueries })
  return nil
end

-- every record, by name and then by address
function config.records(): { rpc.Record }
  local kept = app.kv:get(RECORDS)
  local records: { rpc.Record } = {}
  for _, record in if type(kept) == "table" then kept else {} do
    table.insert(records, {
      id = record.id,
      name = record.name,
      address = record.address,
      subdomains = record.subdomains == true,
    })
  end
  table.sort(records, function(a, b)
    return if a.name == b.name then a.address < b.address else a.name < b.name
  end)
  return records
end

-- Give a name an address. Answers with what is wrong when it cannot.
function config.add(name: string, address: string, subdomains: boolean): string?
  local named, name_problem = dnsmasq.name(name)
  if not named then
    return name_problem
  end
  local addressed, address_problem = dnsmasq.address(address)
  if not addressed then
    return address_problem
  end
  local records = config.records()
  if #records >= MAX_RECORDS then
    return `There can be {MAX_RECORDS} records at most.`
  end
  for _, record in records do
    if record.name == named and record.address == addressed then
      return `{named} has the address {addressed} already.`
    end
  end
  local kept = app.kv:get(NEXT)
  local id = if type(kept) == "number" then kept else 1
  table.insert(records, { id = id, name = named, address = addressed, subdomains = subdomains })
  app.kv:set(NEXT, id + 1)
  app.kv:set(RECORDS, records)
  return nil
end

-- Take records away: every one `gone` says yes to. Answers with how many went.
function config.remove(gone: (rpc.Record) -> boolean): number
  local staying: { rpc.Record } = {}
  local records = config.records()
  for _, record in records do
    if not gone(record) then
      table.insert(staying, record)
    end
  end
  if #staying < #records then
    app.kv:set(RECORDS, staying)
  end
  return #records - #staying
end

return config
