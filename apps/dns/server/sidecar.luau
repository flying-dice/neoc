--!strict
-- The server's container: the one thing this app has in Docker, kept by the container framework (`vos:docker`).
-- What is this app's own is what the container is made from: dnsmasq (dnsmasq.luau) with the configuration
-- (config.luau) as its arguments. A change of the configuration is the container made again, which loses nothing,
-- because the container keeps nothing (dnsmasq's cache aside).
--
-- A function that does something answers with a plain sentence when it could not, and with nothing when it did.
local config = require("./config")
local dnsmasq = require("./dnsmasq")
local docker = require("vos:docker")
local rpc = require("../.app/rpc")

local sidecar = {}

local container = docker.container("vos-dns")

-- Docker's filters for what happens to the container
sidecar.FILTERS = container.filters

-- what the container is made from now
local function declared(): vos_docker_Declaration
  local settings = config.settings()
  return {
    image = dnsmasq.IMAGE,
    entrypoint = dnsmasq.ENTRYPOINT,
    command = dnsmasq.arguments(settings, config.records()),
    -- the same port for UDP (a question) and TCP (an answer too long for one packet)
    ports = {
      { host = settings.port, container = dnsmasq.PORT, udp = true },
      { host = settings.port, container = dnsmasq.PORT, udp = false },
    },
  }
end

-- How the server is doing. With Docker away there is nothing to say of its container.
function sidecar.server(): rpc.Server
  local status = container:status(declared())
  return {
    enabled = config.enabled(),
    state = status.state,
    status = status.status,
    current = status.current,
    image = dnsmasq.IMAGE,
    problem = status.problem,
  }
end

-- Make the container from the configuration as it stands, and start it. What Docker refused is kept, for
-- `sidecar.server` to say, and answered.
function sidecar.apply(): string?
  return container:apply(declared(), true)
end

-- Remove the container. The configuration stays.
function sidecar.remove(): string?
  return container:apply(nil)
end

-- The end of what the server wrote, its output and its errors together in the order Docker has them.
function sidecar.logs(tail: number?): (vos_docker_Logs?, string?)
  local logs, refused = container:logs(tail)
  return logs, if logs then nil else refused or "The server has no container: it is off."
end

return sidecar
