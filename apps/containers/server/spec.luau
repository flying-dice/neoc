--!strict
-- A container's spec: the settings a container is made with, as the form has them. Here it is checked before Docker
-- hears of it (what is wrong is said in a sentence that names the row), turned into Docker's create body, and read
-- back from what Docker says of a container, for the form that changes or copies it. Nothing here talks to Docker.

local spec = {}

export type Spec = containers_Spec

local RESTARTS: { [string]: boolean } = { no = true, always = true, ["unless-stopped"] = true, ["on-failure"] = true }
-- Docker's own rule for the names of containers and volumes
local NAME = "^[%w][%w_%.%-]*$"

-- what a value holds that Docker would read as something else: a NUL ends a C string
local function hasNul(text: string): boolean
  return string.find(text, "\0", 1, true) ~= nil
end

-- what is typed, without the spaces around it
function spec.trim(text: string): string
  return string.match(text, "^%s*(.-)%s*$") :: string
end
local trim = spec.trim

local function isPath(path: string): boolean
  return string.sub(path, 1, 1) == "/"
end

-- a whole path of the machine: Docker's own on Linux, or a drive's on Windows
local function isMachinePath(path: string): boolean
  return isPath(path) or string.match(path, "^%a:[/\\]") ~= nil
end

-- Words as a shell splits them: by spaces, with '...' taken as it is and "..." with \" and \\ in it. Answers with the
-- words, or with nothing when a quote is not closed.
function spec.words(text: string): { string }?
  local words: { string } = {}
  local word: { string }? = nil
  local i = 1
  local n = #text
  while i <= n do
    local c = string.sub(text, i, i)
    if string.match(c, "%s") then
      if word then
        table.insert(words, table.concat(word))
        word = nil
      end
      i += 1
    else
      local current: { string } = word or {}
      word = current
      if c == "'" then
        local close = string.find(text, "'", i + 1, true)
        if not close then
          return nil
        end
        table.insert(current, string.sub(text, i + 1, close - 1))
        i = close + 1
      elseif c == '"' then
        i += 1
        local closed = false
        while i <= n do
          local d = string.sub(text, i, i)
          if d == '"' then
            closed = true
            i += 1
            break
          elseif d == "\\" and i < n and string.find('"\\$`', string.sub(text, i + 1, i + 1), 1, true) then
            table.insert(current, string.sub(text, i + 1, i + 1))
            i += 2
          else
            table.insert(current, d)
            i += 1
          end
        end
        if not closed then
          return nil
        end
      elseif c == "\\" and i < n then
        table.insert(current, string.sub(text, i + 1, i + 1))
        i += 2
      else
        table.insert(current, c)
        i += 1
      end
    end
  end
  if word then
    table.insert(words, table.concat(word))
  end
  return words
end

-- Words as one line that `spec.words` splits back into them.
function spec.line(words: { string }): string
  local quoted: { string } = {}
  for _, word in words do
    if word ~= "" and not string.find(word, "[^%w_%./:=@,+%%%-]") then
      table.insert(quoted, word)
    elseif not string.find(word, "'", 1, true) then
      table.insert(quoted, `'{word}'`)
    else
      table.insert(quoted, '"' .. string.gsub(word, '[\\"$`]', "\\%0") .. '"')
    end
  end
  return table.concat(quoted, " ")
end

-- a part of an image's path: lower-case letters and digits, with ., _, __ or dashes between them
local function pathPart(part: string): boolean
  if
    not string.match(part, "^[a-z0-9]")
    or not string.match(part, "[a-z0-9]$")
    or string.find(part, "[^a-z0-9%._%-]")
  then
    return false
  end
  for separator in string.gmatch(part, "[%._%-]+") do
    if not (separator == "." or separator == "_" or separator == "__" or string.match(separator, "^%-+$")) then
      return false
    end
  end
  return true
end

-- a registry: a host name (labels of letters, digits and dashes, between dots), and a port
local function registry(part: string): boolean
  local host, port = string.match(part, "^([^:]+):(%d+)$")
  host = host or part
  if port == nil and string.find(part, ":", 1, true) then
    return false
  end
  for label in string.gmatch(host :: string, "[^.]+") do
    if not (string.match(label, "^%w$") or string.match(label, "^%w[%w%-]*%w$")) then
      return false
    end
  end
  return not string.find(host :: string, "..", 1, true) and string.sub(host :: string, 1, 1) ~= "."
end

-- Whether `image` is a reference Docker can pull: `[registry/]path[:tag][@digest]`, as Docker reads one.
function spec.isImage(image: string): boolean
  if image == "" or #image > 512 then
    return false
  end
  local rest = image
  local at = string.find(rest, "@", 1, true)
  if at then
    if
      not string.match(
        string.sub(rest, at + 1),
        "^[a-z0-9]+[a-z0-9+%._%-]*:%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x+$"
      )
    then
      return false
    end
    rest = string.sub(rest, 1, at - 1)
  end
  local parts: { string } = string.split(rest, "/")
  local last = parts[#parts]
  local colon = string.find(last, ":", 1, true)
  if colon then
    if not string.match(string.sub(last, colon + 1), "^[%w_][%w_%.%-]*$") or #last - colon > 128 then
      return false
    end
    parts[#parts] = string.sub(last, 1, colon - 1)
  end
  local first = 1
  -- the first part is a registry when it looks like a host: Docker Hub's names have neither dots nor ports
  if #parts > 1 and (string.find(parts[1], "[.:]") or parts[1] == "localhost" or string.find(parts[1], "%u")) then
    if not registry(parts[1]) then
      return false
    end
    first = 2
  end
  for i = first, #parts do
    if not pathPart(parts[i]) then
      return false
    end
  end
  return true
end

-- The image as Docker lists it: `nginx` is `nginx:latest`, which is what a pull of it pulls.
function spec.tagged(image: string): string
  local last = string.match(image, "[^/]*$") or image
  if string.find(image, "@", 1, true) or string.find(last, ":", 1, true) then
    return image
  end
  return `{image}:latest`
end

local function portNumber(text: string): number?
  if not string.match(text, "^%d+$") then
    return nil
  end
  local number = tonumber(text)
  return if number and number >= 1 and number <= 65535 then number else nil
end

-- an address of the machine, as Docker takes one for a port: IPv4's four numbers, or IPv6's hex and colons
local function isAddress(text: string): boolean
  if string.match(text, "^%d+%.%d+%.%d+%.%d+$") then
    for part in string.gmatch(text, "%d+") do
      if (tonumber(part) :: number) > 255 then
        return false
      end
    end
    return true
  end
  return string.find(text, ":", 1, true) ~= nil and not string.find(text, "[^%x:.]")
end

-- the n-th row of a list, as a sentence names it
local function nth(kind: string, i: number, count: number): string
  return if count == 1 then kind else `{kind} {i}`
end

local function entries(list: { containers_Entry }, kind: string, what: string): string?
  local seen: { [string]: boolean } = {}
  for i, entry in list do
    local name = trim(entry.name)
    local which = nth(kind, i, #list)
    if name == "" then
      return `{which} has no name.`
    end
    if hasNul(name .. entry.value) then
      return `{which} has a character Docker cannot take.`
    end
    if what == "environment" and string.find(name, "=", 1, true) then
      return `{which}: a variable's name cannot have = in it.`
    end
    if seen[name] then
      return `The {what} has {name} twice.`
    end
    seen[name] = true
  end
  return nil
end

-- What is wrong with a spec, as a sentence; nothing when Docker can be asked for it.
function spec.refused(asked: Spec): string?
  local image = trim(asked.image)
  if image == "" then
    return "Say which image the container runs, like nginx:1.27."
  end
  if not spec.isImage(image) then
    return `{image} is not the name of an image. An image is named like nginx, nginx:1.27 or ghcr.io/owner/app:1.0.`
  end
  local name = trim(asked.name)
  if name ~= "" and (not string.match(name, NAME) or #name < 2) then
    return "A container's name starts with a letter or a digit, and has only letters, digits, _, . and -."
  end
  if not spec.words(asked.command) then
    return "The command has a quote that is not closed."
  end
  if not spec.words(asked.entrypoint) then
    return "The entrypoint has a quote that is not closed."
  end
  if hasNul(asked.command .. asked.entrypoint .. asked.user .. asked.hostname .. asked.working_dir) then
    return "A setting has a character Docker cannot take."
  end
  local wrong = entries(asked.environment, "Variable", "environment") or entries(asked.labels, "Label", "labels")
  if wrong then
    return wrong
  end

  local published: { [string]: boolean } = {}
  for i, port in asked.ports do
    local which = nth("Port", i, #asked.ports)
    if not portNumber(trim(port.container_port)) then
      return `{which}: the container's port must be a number from 1 to 65535.`
    end
    local host = trim(port.host_port)
    if host ~= "" and not portNumber(host) then
      return `{which}: the machine's port must be a number from 1 to 65535, or empty for one Docker chooses.`
    end
    local ip = trim(port.host_ip)
    if ip ~= "" and not isAddress(ip) then
      return `{which}: {ip} is not an address of the machine, like 127.0.0.1.`
    end
    if port.protocol ~= "tcp" and port.protocol ~= "udp" then
      return `{which}: a port is TCP or UDP.`
    end
    if host ~= "" then
      local key = `{if ip == "" then "0.0.0.0" else ip}:{tonumber(host)}/{port.protocol}`
      if published[key] then
        return `The machine's port {tonumber(host)}/{port.protocol}{if ip == "" then "" else ` on {ip}`} is published twice.`
      end
      published[key] = true
    end
  end

  local targets: { [string]: boolean } = {}
  for i, mount in asked.mounts do
    local which = nth("Mount", i, #asked.mounts)
    local source, target = trim(mount.source), trim(mount.target)
    if hasNul(source .. target) then
      return `{which} has a character Docker cannot take.`
    end
    if mount.kind == "bind" then
      if not isMachinePath(source) then
        return `{which}: a folder of the machine is given by its whole path, like /srv/www.`
      end
    elseif mount.kind == "volume" then
      if source ~= "" and (not string.match(source, NAME) or #source < 2) then
        return `{which}: a volume's name starts with a letter or a digit, and has only letters, digits, _, . and -.`
      end
    else
      return `{which} is a folder of the machine or a volume.`
    end
    if not isPath(target) then
      return `{which}: the place in the container is a whole path, like /data.`
    end
    if targets[target] then
      return `Two mounts are at {target}.`
    end
    targets[target] = true
  end

  if trim(asked.network) == "" then
    return "Say which network the container is put on: bridge is Docker's own."
  end
  if not RESTARTS[asked.restart] then
    return "A restart policy is no, always, unless-stopped or on-failure."
  end
  local retries = trim(asked.retries)
  if asked.restart == "on-failure" and retries ~= "" and not string.match(retries, "^%d+$") then
    return "The number of retries is a whole number, or empty for no limit."
  end
  local dir = trim(asked.working_dir)
  if dir ~= "" and not isPath(dir) then
    return "The working folder is a whole path, like /app."
  end
  return nil
end

-- Docker's create body for a spec that `spec.refused` took.
function spec.body(asked: Spec): vnd_bollard_ContainerCreateBody
  local function orNil(text: string): string?
    local trimmed = trim(text)
    return if trimmed == "" then nil else trimmed
  end
  local function wordsOf(text: string): { string }?
    local words = spec.words(text)
    return if words and #words > 0 then words else nil
  end

  local env: { string } = {}
  for _, entry in asked.environment do
    table.insert(env, `{trim(entry.name)}={entry.value}`)
  end
  local labels: { [string]: string } = {}
  for _, entry in asked.labels do
    labels[trim(entry.name)] = entry.value
  end

  local exposed: { [string]: { [string]: nil } } = {}
  local bindings: { [string]: { vnd_bollard_PortBinding } } = {}
  for _, port in asked.ports do
    local key = `{tonumber(trim(port.container_port))}/{port.protocol}`
    exposed[key] = {}
    local list = bindings[key] or {}
    bindings[key] = list
    local host = trim(port.host_port)
    table.insert(list, {
      HostIp = orNil(port.host_ip),
      HostPort = if host == "" then "" else tostring(tonumber(host)),
    })
  end

  local mounts: { vnd_bollard_Mount } = {}
  for _, mount in asked.mounts do
    local source = trim(mount.source)
    table.insert(mounts, {
      Type = mount.kind,
      -- a volume without a name is an anonymous one, which Docker names
      Source = if source == "" then nil else source,
      Target = trim(mount.target),
      ReadOnly = mount.read_only,
    })
  end

  local retries = tonumber(trim(asked.retries)) or 0
  return {
    Image = trim(asked.image),
    Cmd = wordsOf(asked.command),
    Entrypoint = wordsOf(asked.entrypoint),
    Env = env,
    Labels = labels,
    ExposedPorts = exposed,
    WorkingDir = orNil(asked.working_dir),
    User = orNil(asked.user),
    Hostname = orNil(asked.hostname),
    HostConfig = {
      PortBindings = bindings,
      PublishAllPorts = asked.publish_all,
      Mounts = mounts,
      NetworkMode = trim(asked.network),
      -- Docker takes a number of retries with on-failure only
      RestartPolicy = {
        Name = asked.restart :: vnd_bollard_RestartPolicyNameEnum,
        MaximumRetryCount = if asked.restart == "on-failure" then retries else nil,
      },
    },
  }
end

-- `src:dst[:options]`, a bind of the older kind: a source that is a path is a folder of the machine, else a volume
local function bindOf(bind: string): containers_Mount?
  local drive = string.match(bind, "^%a:[/\\]") and string.sub(bind, 1, 2) or ""
  local parts: { string } = string.split(string.sub(bind, #drive + 1), ":")
  if #parts < 2 then
    return nil
  end
  local source = drive .. parts[1]
  local options = parts[3] or ""
  local readOnly = false
  for option in string.gmatch(options, "[^,]+") do
    readOnly = readOnly or option == "ro"
  end
  return {
    kind = if isMachinePath(source) then "bind" else "volume",
    source = source,
    target = parts[2],
    read_only = readOnly,
  }
end

-- The spec of a container that is there, from Docker's inspection of it: what the form changes or copies. What
-- Docker filled in itself (the host name it made from the id, a network mode of `default`) is left empty, so the
-- container made from it gets Docker's own again.
function spec.of(found: vnd_bollard_ContainerInspectResponse): Spec
  local config: vnd_bollard_ContainerConfig = found.Config or {}
  local host: vnd_bollard_HostConfig = found.HostConfig or {}

  local environment: { containers_Entry } = {}
  local variables: { string } = config.Env or {}
  for _, variable in variables do
    local name, value = string.match(variable, "^([^=]*)=?(.*)$")
    table.insert(environment, { name = name or variable, value = value or "" })
  end
  local labels: { containers_Entry } = {}
  local labelled: { [string]: string } = config.Labels or {}
  for name, value in labelled do
    table.insert(labels, { name = name, value = value })
  end
  table.sort(labels, function(a, b)
    return a.name < b.name
  end)

  local ports: { containers_Port } = {}
  local bound: { [string]: { vnd_bollard_PortBinding } } = host.PortBindings or {}
  for key, bindings in bound do
    local number, protocol = string.match(key, "^(%d+)/?(%a*)$")
    if number then
      for _, binding in bindings do
        table.insert(ports, {
          host_ip = binding.HostIp or "",
          host_port = binding.HostPort or "",
          container_port = number,
          protocol = if protocol == "udp" then "udp" else "tcp",
        })
      end
    end
  end
  table.sort(ports, function(a, b)
    local x, y = tonumber(a.container_port) or 0, tonumber(b.container_port) or 0
    if x ~= y then
      return x < y
    end
    return if a.protocol ~= b.protocol then a.protocol < b.protocol else a.host_port < b.host_port
  end)

  local mounts: { containers_Mount } = {}
  local binds: { string } = host.Binds or {}
  for _, bind in binds do
    local mount = bindOf(bind)
    if mount then
      table.insert(mounts, mount)
    end
  end
  local declared: { vnd_bollard_Mount } = host.Mounts or {}
  for _, mount in declared do
    if mount.Type == "bind" or mount.Type == "volume" then
      table.insert(mounts, {
        kind = mount.Type,
        source = mount.Source or "",
        target = mount.Target or "",
        read_only = mount.ReadOnly == true,
      })
    end
  end

  local network = host.NetworkMode or ""
  local policy: vnd_bollard_RestartPolicy = host.RestartPolicy or {}
  local restart = policy.Name or ""
  local retries = policy.MaximumRetryCount or 0
  local hostname = config.Hostname or ""
  return {
    image = config.Image or found.Image or "",
    name = string.gsub(found.Name or "", "^/", ""),
    command = spec.line(config.Cmd or {}),
    entrypoint = spec.line(config.Entrypoint or {}),
    environment = environment,
    ports = ports,
    publish_all = host.PublishAllPorts == true,
    mounts = mounts,
    network = if network == "" or network == "default" then "bridge" else network,
    restart = if RESTARTS[restart] then restart :: containers_Restart else "no",
    retries = if restart == "on-failure" and retries > 0 then tostring(retries) else "",
    labels = labels,
    working_dir = config.WorkingDir or "",
    user = config.User or "",
    -- Docker names a container's host after the start of its id
    hostname = if found.Id and hostname == string.sub(found.Id, 1, 12) then "" else hostname,
    start = found.State ~= nil and found.State.Running == true,
  }
end

return spec
