--!strict
-- What a container an app owns is made from, as the app declares it: checked before Docker hears of it, turned into
-- Docker's own words, and summed up in one short word (its signature). Nothing here talks to Docker or keeps
-- anything: `owned.luau` does.
local hash = require("lib:hash")
local hex = require("lib:hex")

local declaration = {}

-- How the framework makes a container, beyond what is declared (read-only folders, the restart policy). It is part
-- of the signature: change it with `body`, and containers made the old way are made again.
local MADE = "1"
-- the most of each that one container is declared with
local MAX_WORDS = 4096
local MAX_PORTS = 256
local MAX_MOUNTS = 256

-- A port of the container, published on the machine's `host` port: TCP, or UDP when `udp`.
export type Port = vos_docker_Port
-- A folder of the machine (`source`, an absolute path) the container may read at `target`. It is never writable.
export type Mount = vos_docker_Mount
-- What the container is made from. Everything it gets of the machine is here: nothing is published or mounted that
-- is not declared.
export type Declaration = vos_docker_Declaration
-- Whether `list` is a list of at most `limit`: a table with any other key is not one, and Docker would be sent it
-- as something else than was declared.
local function listed(list: any, limit: number): boolean
  if type(list) ~= "table" or #list > limit then
    return false
  end
  local count = 0
  for _ in list do
    count += 1
  end
  return count == #list
end

local function words(list: any, limit: number): boolean
  if list == nil then
    return true
  end
  if not listed(list, limit) then
    return false
  end
  for _, word in list do
    -- a NUL ends a C string: Docker would run something else than was declared
    if type(word) ~= "string" or string.find(word, "\0", 1, true) then
      return false
    end
  end
  return true
end

local function portNumber(port: any): boolean
  return type(port) == "number" and port == math.floor(port) and port >= 1 and port <= 65535
end

-- What is wrong with a declaration, as a sentence; nothing when Docker can be asked for it.
function declaration.refused(declared: Declaration): string?
  if type(declared) ~= "table" then
    return "A container is declared with a table."
  end
  if type(declared.image) ~= "string" or declared.image == "" or string.find(declared.image, "[%s%z]") then
    return "A container is declared with the name of its image."
  end
  if not words(declared.entrypoint, MAX_WORDS) or not words(declared.command, MAX_WORDS) then
    return "What a container runs is a list of words."
  end
  if not words(declared.environment, MAX_WORDS) then
    return "A container's environment is a list of NAME=value."
  end
  local environment: { string } = declared.environment or {}
  for _, variable in environment do
    if not string.match(variable, "^[^=]+=") then
      return "A container's environment is a list of NAME=value."
    end
  end
  local ports: any = declared.ports or {}
  if not listed(ports, MAX_PORTS) then
    return "A container's ports are a list."
  end
  for _, port in ports do
    if
      type(port) ~= "table"
      or not portNumber(port.host)
      or not portNumber(port.container)
      or not (port.udp == nil or type(port.udp) == "boolean")
    then
      return "A port is a number from 1 to 65535, and `udp` is true or false."
    end
  end
  local mounts: any = declared.mounts or {}
  if not listed(mounts, MAX_MOUNTS) then
    return "A container's folders are a list."
  end
  for _, mount in mounts do
    if
      type(mount) ~= "table"
      or type(mount.source) ~= "string"
      or type(mount.target) ~= "string"
      or string.find(mount.source .. mount.target, "\0", 1, true)
      -- a name that is no path would be a volume of Docker's, which nothing declared
      or not (string.match(mount.source, "^/") or string.match(mount.source, "^%a:[/\\]"))
      or not string.match(mount.target, "^/")
    then
      return "A folder is given by its whole path, and a place in the container that starts with /."
    end
  end
  return nil
end

-- the port bindings, by Docker's key (`53/udp`), in a stable order
local function published(declared: Declaration): ({ string }, { [string]: any })
  local keys: { string } = {}
  local bindings: { [string]: any } = {}
  local ports: { Port } = declared.ports or {}
  for _, port in ports do
    local key = `{port.container}/{if port.udp then "udp" else "tcp"}`
    if not bindings[key] then
      table.insert(keys, key)
      bindings[key] = {}
    end
    table.insert(bindings[key], { HostPort = tostring(port.host) })
  end
  table.sort(keys)
  return keys, bindings
end

-- What a container is made from, as one short word: two containers with the same signature are the same.
function declaration.signature(declared: Declaration): string
  -- each part with its length, so that no two declarations spell the same
  local parts: { string } = {}
  local function add(kind: string, values: { string })
    table.insert(parts, `{kind}:{#values}`)
    for _, value in values do
      table.insert(parts, `{#value}:{value}`)
    end
  end
  add("made", { MADE })
  add("image", { declared.image })
  add("entrypoint", declared.entrypoint or {})
  add("command", declared.command or {})
  add("environment", declared.environment or {})
  local keys, bindings = published(declared)
  for _, key in keys do
    local hosts: { string } = {}
    for _, binding in bindings[key] do
      table.insert(hosts, binding.HostPort)
    end
    add(`port {key}`, hosts)
  end
  local folders: { Mount } = declared.mounts or {}
  for _, mount in folders do
    add("mount", { mount.source, mount.target })
  end
  return hex.encode(hash.hash("sha256", table.concat(parts, "\n")))
end

-- Docker's create body for the container: what was declared and nothing else. Every folder is read-only, and
-- nothing is published that is not a declared port.
function declaration.body(declared: Declaration, labels: { [string]: string }): { [string]: any }
  local _, bindings = published(declared)
  -- the folders are only read: nothing the container does can change the person's files
  local mounts: { any } = {}
  local folders: { Mount } = declared.mounts or {}
  for _, mount in folders do
    table.insert(mounts, { Type = "bind", Source = mount.source, Target = mount.target, ReadOnly = true })
  end
  return {
    Image = declared.image,
    Entrypoint = declared.entrypoint,
    Cmd = declared.command,
    Env = declared.environment,
    Labels = labels,
    HostConfig = {
      PortBindings = bindings,
      Mounts = mounts,
      -- it comes back with Docker, and after a crash; not after it was stopped by hand
      RestartPolicy = { Name = "unless-stopped" },
    },
  }
end

return declaration
