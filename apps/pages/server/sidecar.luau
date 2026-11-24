--!strict
-- The server's container: the one thing this app has in Docker. It is made from the configuration (config.luau) and
-- never changed: a change of the configuration is the container removed and made again, which takes a moment and
-- loses nothing, because the container keeps nothing. So it is always what its label says it was made from, and "is
-- it up to date" is one comparison. With no site to serve there is no container.
--
-- A function that does something answers with a plain sentence when it could not, and with nothing when it did.
local config = require("./config")
local engine = require("./engine")
local fs = require("std:fs")
local nginx = require("./nginx")
local rpc = require("../.app/rpc")
local text = require("./text")

local sidecar = {}

local NAME = "vos-pages"
-- the label that says a container is this app's, and the one that says what it was made from
local OWNER, OWNED = "vos.app", "pages"
local SIGNATURE = "vos.pages.signature"
-- the most of the server's output that is read and sent, in bytes
local LOG_LIMIT = 256 * 1024
local DEFAULT_TAIL = 200

-- Docker's filter for this app's containers, and for what happens to them
sidecar.FILTER = { label = { `{OWNER}={OWNED}` } }
-- what changes how the container is doing; the rest of what Docker reports does not
sidecar.EVENTS = { "create", "start", "die", "destroy", "pause", "unpause", "restart" }

-- why the container could not be made or started the last time it was tried; empty when it could
local problem = ""

-- What is known to be wrong with a site's folder. Docker would refuse the whole container for one folder that is
-- not there, so such a site is left out of it instead. Only this machine's folders can be looked at: with another
-- Docker chosen the folders are that machine's, and nothing is known.
local function unusable(site: rpc.Site): string
  if engine.host() ~= "" then
    return ""
  end
  local found = fs.metadata(site.folder)
  if not found then
    return "The folder is not there."
  end
  return if found.kind == "dir" then "" else "That is a file, not a folder."
end

-- Every site, each with what is known to be wrong with it when it is on.
function sidecar.sites(): { rpc.Site }
  local sites = config.sites()
  for _, site in sites do
    site.problem = if site.enabled then unusable(site) else ""
  end
  return sites
end

-- what the container is made from now, and the signature of that
local function wanted(): (nginx.Plan, string)
  local served: { rpc.Site } = {}
  for _, site in sidecar.sites() do
    if site.enabled and site.problem == "" then
      table.insert(served, site)
    end
  end
  local plan = nginx.plan(config.settings(), served)
  return plan, nginx.signature(plan)
end

-- this app's containers, as Docker lists them: one, or none (more only if something went wrong half way)
local function owned(api: engine.Client): ({ vnd_bollard_ContainerSummary }?, string?)
  local list, err = api:list_containers { all = true, filters = sidecar.FILTER }
  if err then
    return nil, engine.sentence(err)
  end
  return list, nil
end

local function isWanted(): boolean
  for _, site in config.sites() do
    if site.enabled then
      return true
    end
  end
  return false
end

local function absent(): rpc.Server
  return {
    wanted = isWanted(),
    state = "absent",
    status = "",
    current = false,
    image = nginx.IMAGE,
    problem = problem,
  }
end

-- How the server is doing. With Docker away there is nothing to say of its container.
function sidecar.server(): rpc.Server
  local api = engine.reader()
  local list = if api then owned(api) else nil
  local found = if list then list[1] else nil
  if not found then
    return absent()
  end
  local _, signature = wanted()
  local state: rpc.ServerState = if found.State == "running"
    then "running"
    elseif found.State == "restarting" then "restarting"
    else "stopped"
  local server = absent()
  server.state = state
  server.status = found.Status or ""
  server.current = (found.Labels or {})[SIGNATURE] == signature
  return server
end

-- have the image, pulling it when Docker does not
local function image(api: engine.Client): string?
  local images, err = api:list_images { filters = { reference = { nginx.IMAGE } } }
  if err then
    return engine.sentence(err)
  end
  if images and #images > 0 then
    return nil
  end
  local puller, unreachable = engine.puller()
  if not puller then
    return unreachable
  end
  local pull = puller:pull_image(nginx.IMAGE)
  -- the pull runs as its progress is read: an image that cannot be pulled is the error of a step
  while true do
    local step, step_err = pull:next()
    if step_err then
      return `The image {nginx.IMAGE} could not be pulled: {engine.sentence(step_err)}`
    end
    if not step then
      return nil
    end
  end
end

local function removeAll(api: engine.Client): string?
  local list, unlisted = owned(api)
  if not list then
    return unlisted
  end
  local failure: string? = nil
  for _, found in list do
    if found.Id then
      local _, err = api:remove_container(found.Id, { force = true })
      failure = failure or (if err then engine.sentence(err) else nil)
    end
  end
  return failure
end

-- Whether the one container there is was made from `signature` and runs: there is nothing to make then.
local function standing(api: engine.Client, signature: string): boolean
  local list = owned(api)
  local found = if list and #list == 1 then list[1] else nil
  return found ~= nil and found.State == "running" and (found.Labels or {})[SIGNATURE] == signature
end

local function make(api: engine.Client, always: boolean): string?
  local plan, signature = wanted()
  if #plan.mounts == 0 then
    return removeAll(api)
  end
  if not always and standing(api, signature) then
    return nil
  end
  local missing = image(api)
  if missing then
    return missing
  end
  local stale = removeAll(api)
  if stale then
    return stale
  end
  -- every port nginx listens on, published under its own number
  local published: { [string]: { vnd_bollard_PortBinding } } = {}
  for _, port in plan.ports do
    published[`{port}/tcp`] = { { HostPort = tostring(port) } }
  end
  -- the folders are only read: nothing a site's visitor does can change the person's files
  local mounts: { vnd_bollard_Mount } = {}
  for _, mount in plan.mounts do
    table.insert(mounts, { Type = "bind", Source = mount.source, Target = mount.target, ReadOnly = true })
  end
  local made, err = api:create_container({
    Image = nginx.IMAGE,
    Entrypoint = nginx.ENTRYPOINT,
    Env = { `{nginx.CONFIG}={plan.config}` },
    Labels = { [OWNER] = OWNED, [SIGNATURE] = signature },
    HostConfig = {
      PortBindings = published,
      Mounts = mounts,
      -- it comes back with Docker, and after a crash; not after it was stopped by hand
      RestartPolicy = { Name = "unless-stopped" },
    },
  }, { name = NAME })
  if err then
    return engine.sentence(err)
  end
  assert(made)
  local _, start_err = api:start_container(made.Id)
  return if start_err then engine.sentence(start_err) else nil
end

-- Have the container be what the configuration says as it stands: made and started when there is a site to serve,
-- removed when there is none. A container that is that already is left running, unless `always`. What Docker
-- refused is kept, for `sidecar.server` to say, and answered.
function sidecar.apply(always: boolean): string?
  local api, unreachable = engine.actor()
  problem = (if api then make(api, always) else unreachable) or ""
  return if problem == "" then nil else problem
end

-- The end of what the server wrote, its output and its errors together in the order Docker has them.
function sidecar.logs(tail: number?): (rpc.Logs?, string?)
  local api, unreachable = engine.reader()
  if not api then
    return nil, unreachable
  end
  local list, unlisted = owned(api)
  if not list then
    return nil, unlisted
  end
  local id = list[1] and list[1].Id
  if not id then
    return nil, "The server has no container: no site is on."
  end
  local lines = tail or DEFAULT_TAIL
  local stream = api:logs(id, { tail = lines })
  local pieces: { string } = {}
  while true do
    local piece, piece_err = stream:next()
    if piece_err then
      return nil, engine.sentence(piece_err)
    end
    if not piece then
      break
    end
    table.insert(pieces, piece.data)
  end
  local written, cut = text.tail(table.concat(pieces), LOG_LIMIT)
  local _, breaks = string.gsub(written, "\n", "")
  return { text = written, truncated = cut or breaks >= lines }, nil
end

return sidecar
