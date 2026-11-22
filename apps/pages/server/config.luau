--!strict
-- What the server is made from, kept by the app: the port the named sites share, and the sites. This is the only
-- copy (the container is made from it, never read back), so it is there with Docker away, and it is the same server
-- in whichever Docker it is started.
local app = require("vos:app")
local nginx = require("./nginx")
local rpc = require("../.app/rpc")

local config = {}

local SETTINGS = "settings"
local SITES = "sites"
-- the id the next site gets
local NEXT = "next"

local MAX_SITES = 100
local MAX_HOSTNAMES = 20
local MAX_NAME = 80
local MAX_FOLDER = 1024
local DEFAULTS: rpc.Settings = { port = 80 }

-- what a person gives for a site, checked and as it is kept
type Given = { name: string, folder: string, port: number, hostnames: { string } }

function config.settings(): rpc.Settings
  local kept = app.kv:get(SETTINGS)
  if type(kept) ~= "table" then
    return table.clone(DEFAULTS)
  end
  return { port = if type(kept.port) == "number" then kept.port else DEFAULTS.port }
end

-- every site, by name; `problem` is for whoever knows of one (sidecar.luau)
function config.sites(): { rpc.Site }
  local kept = app.kv:get(SITES)
  local sites: { rpc.Site } = {}
  for _, site in if type(kept) == "table" then kept else {} do
    local hostnames: { string } = {}
    for _, hostname in if type(site.hostnames) == "table" then site.hostnames else {} do
      table.insert(hostnames, tostring(hostname))
    end
    table.insert(sites, {
      id = site.id,
      name = site.name,
      folder = site.folder,
      port = if type(site.port) == "number" then site.port else 0,
      hostnames = hostnames,
      enabled = site.enabled == true,
      problem = "",
    })
  end
  table.sort(sites, function(a, b)
    local left, right = string.lower(a.name), string.lower(b.name)
    return if left == right then a.id < b.id else left < right
  end)
  return sites
end

local function keep(sites: { rpc.Site })
  local kept: { any } = {}
  for _, site in sites do
    table.insert(kept, {
      id = site.id,
      name = site.name,
      folder = site.folder,
      port = site.port,
      hostnames = site.hostnames,
      enabled = site.enabled,
    })
  end
  app.kv:set(SITES, kept)
end

-- Keep the port the named sites share. Answers with what is wrong with it when something is, and keeps nothing
-- then.
function config.saveSettings(port: number): string?
  for _, site in config.sites() do
    if site.port == port then
      return `Port {port} is the own port of {site.name}.`
    end
  end
  app.kv:set(SETTINGS, { port = port })
  return nil
end

-- A folder as it is kept: a full path without the slash at its end. Whether it is there is not asked here: it is a
-- folder of the machine Docker runs on, which may be another one.
local function folderOf(raw: string): (string?, string?)
  local folder = string.match(raw, "^%s*(.-)%s*$") :: string
  if folder == "" then
    return nil, "The folder is blank."
  elseif #folder > MAX_FOLDER then
    return nil, `The folder is longer than {MAX_FOLDER} characters.`
  elseif string.find(folder, "%c") then
    return nil, "The folder has a character in it that no path has."
  elseif string.sub(folder, 1, 1) ~= "/" then
    return nil, "The folder must be a full path, like /Users/me/Sites/blog."
  end
  folder = string.gsub(folder, "/+$", "")
  if folder == "" then
    return nil, "The whole disk cannot be a site: choose a folder in it."
  end
  return folder, nil
end

-- What was given for a site, as it is kept, or what is wrong with it. `except` is the site being changed, which
-- does not clash with itself.
local function checked(given: Given, except: number?): (Given?, string?)
  local name = string.match(given.name, "^%s*(.-)%s*$") :: string
  if name == "" then
    return nil, "The name is blank."
  elseif #name > MAX_NAME then
    return nil, `The name is longer than {MAX_NAME} characters.`
  end
  local folder, unusable = folderOf(given.folder)
  if not folder then
    return nil, unusable
  end
  local hostnames: { string } = {}
  for _, raw in given.hostnames do
    if string.match(raw, "%S") then
      local hostname, problem = nginx.hostname(raw)
      if not hostname then
        return nil, problem
      end
      if not table.find(hostnames, hostname) then
        table.insert(hostnames, hostname)
      end
    end
  end
  if #hostnames > MAX_HOSTNAMES then
    return nil, `A site can have {MAX_HOSTNAMES} hostnames at most.`
  end
  if given.port == 0 and #hostnames == 0 then
    return nil, "A site needs a port, a hostname, or both: there is no other way to reach it."
  end
  if given.port ~= 0 and given.port == config.settings().port then
    return nil, `Port {given.port} is the one the sites with hostnames share. Choose another, or change that one.`
  end
  for _, other in config.sites() do
    if other.id ~= except then
      if given.port ~= 0 and other.port == given.port then
        return nil, `Port {given.port} is the port of {other.name}.`
      end
      for _, hostname in hostnames do
        if table.find(other.hostnames, hostname) then
          return nil, `{hostname} is a hostname of {other.name}.`
        end
      end
    end
  end
  table.sort(hostnames)
  return { name = name, folder = folder, port = given.port, hostnames = hostnames }, nil
end

-- Host a folder as a site, on from the start. Answers with what is wrong when it cannot.
function config.add(given: Given): string?
  local sites = config.sites()
  if #sites >= MAX_SITES then
    return `There can be {MAX_SITES} sites at most.`
  end
  local site, problem = checked(given, nil)
  if not site then
    return problem
  end
  local kept = app.kv:get(NEXT)
  local id = if type(kept) == "number" then kept else 1
  table.insert(sites, {
    id = id,
    name = site.name,
    folder = site.folder,
    port = site.port,
    hostnames = site.hostnames,
    enabled = true,
    problem = "",
  })
  app.kv:set(NEXT, id + 1)
  keep(sites)
  return nil
end

config.NO_SITE = "There is no such site."

-- Change what a site is made of; whether it is on stays. Answers with what is wrong when it cannot.
function config.update(id: number, given: Given): string?
  local sites = config.sites()
  for _, site in sites do
    if site.id == id then
      local changed, problem = checked(given, id)
      if not changed then
        return problem
      end
      site.name, site.folder, site.port, site.hostnames = changed.name, changed.folder, changed.port, changed.hostnames
      keep(sites)
      return nil
    end
  end
  return config.NO_SITE
end

-- Turn a site on or off. Answers with what is wrong when it cannot.
function config.setEnabled(id: number, enabled: boolean): string?
  local sites = config.sites()
  for _, site in sites do
    if site.id == id then
      site.enabled = enabled
      keep(sites)
      return nil
    end
  end
  return config.NO_SITE
end

-- Forget sites: every one `gone` says yes to. Answers with how many went.
function config.remove(gone: (rpc.Site) -> boolean): number
  local staying: { rpc.Site } = {}
  local sites = config.sites()
  for _, site in sites do
    if not gone(site) then
      table.insert(staying, site)
    end
  end
  if #staying < #sites then
    keep(staying)
  end
  return #sites - #staying
end

return config
