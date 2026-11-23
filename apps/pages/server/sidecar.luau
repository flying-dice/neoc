--!strict
-- The server's container: the one thing this app has in Docker, kept by the container framework (`vos:docker`).
-- What is this app's own is what the container is made from: nginx (nginx.luau) with the configuration
-- (config.luau) as its environment, its ports and its folders. A change of the configuration is the container made
-- again, which loses nothing, because the container keeps nothing. With no site to serve there is no container.
--
-- A function that does something answers with a plain sentence when it could not, and with nothing when it did.
local config = require("./config")
local docker = require("vos:docker")
local fs = require("std:fs")
local nginx = require("./nginx")
local rpc = require("../.app/rpc")

local sidecar = {}

local container = docker.container("vos-pages")

-- Docker's filters for what happens to the container
sidecar.FILTERS = container.filters

-- What is known to be wrong with a site's folder. Docker would refuse the whole container for one folder that is
-- not there, so such a site is left out of it instead. Only this machine's folders can be looked at: with another
-- Docker chosen the folders are that machine's, and nothing is known.
local function unusable(site: rpc.Site): string
  if docker.host() ~= "" then
    return ""
  end
  local found = fs.metadata(site.folder)
  if not found then
    return "The folder is not there."
  end
  return if found:is_dir() then "" else "That is a file, not a folder."
end

-- Every site, each with what is known to be wrong with it when it is on.
function sidecar.sites(): { rpc.Site }
  local sites = config.sites()
  for _, site in sites do
    site.problem = if site.enabled then unusable(site) else ""
  end
  return sites
end

-- What the container is made from now; nothing when there is no site to serve.
local function declared(): vos_DockerDeclaration?
  local served: { rpc.Site } = {}
  for _, site in sidecar.sites() do
    if site.enabled and site.problem == "" then
      table.insert(served, site)
    end
  end
  local plan = nginx.plan(config.settings(), served)
  if #plan.mounts == 0 then
    return nil
  end
  -- every port nginx listens on, published under its own number
  local ports: { vos_DockerPort } = {}
  for _, port in plan.ports do
    table.insert(ports, { host = port, container = port })
  end
  return {
    image = nginx.IMAGE,
    entrypoint = nginx.ENTRYPOINT,
    environment = { `{nginx.CONFIG}={plan.config}` },
    ports = ports,
    -- the folders are only read: nothing a site's visitor does can change the person's files
    mounts = plan.mounts,
  }
end

local function isWanted(): boolean
  for _, site in config.sites() do
    if site.enabled then
      return true
    end
  end
  return false
end

-- How the server is doing. With Docker away there is nothing to say of its container.
function sidecar.server(): rpc.Server
  local status = container:status(declared())
  return {
    wanted = isWanted(),
    state = status.state,
    status = status.status,
    current = status.current,
    image = nginx.IMAGE,
    problem = status.problem,
  }
end

-- Have the container be what the configuration says as it stands: made and started when there is a site to serve,
-- removed when there is none. A container that is that already is left running, unless `always`. What Docker
-- refused is kept, for `sidecar.server` to say, and answered.
function sidecar.apply(always: boolean): string?
  return container:apply(declared(), always)
end

-- The end of what the server wrote, its output and its errors together in the order Docker has them.
function sidecar.logs(tail: number?): (vos_DockerLogs?, string?)
  local logs, refused = container:logs(tail)
  return logs, if logs then nil else refused or "The server has no container: no site is on."
end

return sidecar
