--!strict
-- The server's container: the one thing this app has in Docker. It is made from the configuration (config.luau) and
-- never changed: a change of the configuration is the container removed and made again, which takes a moment and
-- loses nothing, because the container keeps nothing (dnsmasq's cache aside). So it is always what its label says it
-- was made from, and "is it up to date" is one comparison.
--
-- A function that does something answers with a plain sentence when it could not, and with nothing when it did.
local config = require("./config")
local dnsmasq = require("./dnsmasq")
local engine = require("./engine")
local rpc = require("../.app/rpc")
local text = require("./text")

local sidecar = {}

local NAME = "vos-dns"
-- the label that says a container is this app's, and the one that says what it was made from
local OWNER, OWNED = "vos.app", "dns"
local SIGNATURE = "vos.dns.signature"
-- the most of the server's output that is read and sent, in bytes
local LOG_LIMIT = 256 * 1024
local DEFAULT_TAIL = 200

-- Docker's filter for this app's containers, and for what happens to them
sidecar.FILTER = { label = { `{OWNER}={OWNED}` } }
-- what changes how the container is doing; the rest of what Docker reports does not
sidecar.EVENTS = { "create", "start", "die", "destroy", "pause", "unpause", "restart" }

-- why the container could not be made or started the last time it was tried; empty when it could
local problem = ""

-- what the container is made from now, and the signature of that
local function wanted(): (rpc.Settings, { string }, string)
  local settings = config.settings()
  local arguments = dnsmasq.arguments(settings, config.records())
  return settings, arguments, dnsmasq.signature(settings.port, arguments)
end

-- this app's containers, as Docker lists them: one, or none (more only if something went wrong half way)
local function owned(api: engine.Client): ({ vnd_bollard_ContainerSummary }?, string?)
  local list, err = api:list_containers { all = true, filters = sidecar.FILTER }
  if err then
    return nil, engine.sentence(err)
  end
  return list, nil
end

local function absent(): rpc.Server
  return {
    enabled = config.enabled(),
    state = "absent",
    status = "",
    current = false,
    image = dnsmasq.IMAGE,
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
  local _, _, signature = wanted()
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
  local images, err = api:list_images { filters = { reference = { dnsmasq.IMAGE } } }
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
  local pull = puller:pull_image(dnsmasq.IMAGE)
  -- the pull runs as its progress is read: an image that cannot be pulled is the error of a step
  while true do
    local step, step_err = pull:next()
    if step_err then
      return `The image {dnsmasq.IMAGE} could not be pulled: {engine.sentence(step_err)}`
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

local function make(api: engine.Client): string?
  local missing = image(api)
  if missing then
    return missing
  end
  local stale = removeAll(api)
  if stale then
    return stale
  end
  local settings, arguments, signature = wanted()
  -- the same port for UDP (a question) and TCP (an answer too long for one packet)
  local published: { vnd_bollard_PortBinding } = { { HostPort = tostring(settings.port) } }
  local made, err = api:create_container({
    Image = dnsmasq.IMAGE,
    Entrypoint = dnsmasq.ENTRYPOINT,
    Cmd = arguments,
    Labels = { [OWNER] = OWNED, [SIGNATURE] = signature },
    HostConfig = {
      PortBindings = { [`{dnsmasq.PORT}/udp`] = published, [`{dnsmasq.PORT}/tcp`] = published },
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

-- Make the container from the configuration as it stands, and start it. What Docker refused is kept, for
-- `sidecar.server` to say, and answered.
function sidecar.apply(): string?
  local api, unreachable = engine.actor()
  problem = (if api then make(api) else unreachable) or ""
  return if problem == "" then nil else problem
end

-- Remove the container. The configuration stays.
function sidecar.remove(): string?
  problem = ""
  local api, unreachable = engine.actor()
  if not api then
    return unreachable
  end
  return removeAll(api)
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
    return nil, "The server has no container: it is off."
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
