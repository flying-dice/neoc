local __DARKLUA_BUNDLE_MODULES={cache={}}do do local function __modImpl()























local strings = require("lib:text")

local text = {}




function text.tail(raw, limit)  
local cut = #raw > limit
  if cut then
    raw = string.sub(raw, #raw - limit + 1)
    
    local newline = string.find(raw, "\n", 1, true)
    if newline then
      raw = string.sub(raw, newline + 1)
    end
  end
  return strings.strip_ansi(strings.to_valid_utf8(raw)), cut
end

return text
end function __DARKLUA_BUNDLE_MODULES.a()local v=__DARKLUA_BUNDLE_MODULES.cache.a if not v then v={c=__modImpl()}__DARKLUA_BUNDLE_MODULES.cache.a=v end return v.c end end do local function __modImpl()





local app = require("vos:app")
local bollard = require("vnd:bollard")
local task = require("std:task")
local text = __DARKLUA_BUNDLE_MODULES.a()
local tracing = require("lib:tracing")

local engine = {}


local HOST = "host"




local READ_TIMEOUT = 5
local ACT_TIMEOUT = 90
local PULL_TIMEOUT = 600

local RETRY = 5

local LOG_LIMIT = 256 * 1024
local DEFAULT_TAIL = 200
local MAX_TAIL = 10000

local HOST_LIMIT = 300











local clients= nil

local watched= {}

local chosen = 0



local was_reachable= nil
local last_error = ""

function engine.host()  
local host = app.kv:get(HOST)
  return if type(host) == "string" then host else ""
end



function engine.sentence(err)  
if not err then
    return "Docker refused, and did not say why."
  end
  if err.kind == "connect" then
    local host = engine.host()
    return if host == ""
      then "Docker is not running on this machine, or cannot be reached."
      else `Docker cannot be reached at {host}.`
  elseif err.kind == "timed_out" then
    return "Docker is not answering. It may still be starting, or be paused."
  end
  return if err.message ~= "" then err.message else "Docker refused, and did not say why."
end

local function connected()  
if clients then
    return clients, nil
  end
  local host = if engine.host() == "" then nil else engine.host()
  local made= {}
  for _, timeout in { READ_TIMEOUT, ACT_TIMEOUT, PULL_TIMEOUT } do
    local client, err = bollard.connect { host = host, timeout = timeout }
    if not client then
      last_error = if err then err.message else ""
      return nil, engine.sentence(err)
    end
    table.insert(made, client)
  end
  clients = { reading = made[1], acting = made[2], pulling = made[3] }
  return clients, nil
end


function engine.reader()  
local all, problem = connected()
  return if all then all.reading else nil, problem
end


function engine.actor()  
local all, problem = connected()
  return if all then all.acting else nil, problem
end


function engine.puller()  
local all, problem = connected()
  return if all then all.pulling else nil, problem
end


local function noted(status)  
if status.reachable ~= was_reachable then
    if not status.reachable then
      tracing.warn(status.problem, { host = status.host, error = last_error })
    elseif was_reachable == false then
      tracing.info("Docker can be reached again", { host = status.host, version = status.version })
    end
    was_reachable = status.reachable
  end
  return status
end


function engine.status()  
local status= { reachable = false, version = "", host = engine.host(), problem = "" }
  local api, problem = engine.reader()
  if not api then
    status.problem = problem or ""
    return noted(status)
  end
  local version, err = api:version()
  if not version then
    last_error = if err then err.message else ""
    status.problem = engine.sentence(err)
    return noted(status)
  end
  status.reachable = true
  status.version = version.Version or ""
  return noted(status)
end




function engine.use(host)  
if type(host) ~= "string" or #host > HOST_LIMIT then
    return "That is not an address of a Docker."
  end
  host = (string.match(host, "^%s*(.-)%s*$") )  
if host ~= "" then
    if
      not (string.match(host, "^unix:///") or string.match(host, "^tcp://[^/]") or string.match(host, "^https?://[^/]"))
    then
      return "The address must start with unix:///, tcp:// or http://, or be empty for this machine's own Docker."
    end
    
    local _, err = bollard.connect { host = host }
    if err and err.kind ~= "connect" then
      return engine.sentence(err)
    end
  end
  if host == "" then
    app.kv:delete(HOST)
  else
    app.kv:set(HOST, host)
  end
  clients = nil
  
  was_reachable = nil
  
  chosen += 1
  for events in watched do
    events:close()
  end
  return nil
end




function engine.logs(container, tail)  
local api, problem = engine.reader()
  if not api then
    return nil, problem
  end
  
  local asked = if type(tail) == "number" and tail == tail then tail else DEFAULT_TAIL
  local lines = math.clamp(math.floor(asked), 1, MAX_TAIL)
  
  local stream = api:logs(container, { tail = lines })
  local pieces= {}
  local read = 0
  while true do
    local piece, piece_err = stream:next()
    if piece_err then
      return nil, engine.sentence(piece_err)
    end
    if not piece then
      break
    end
    table.insert(pieces, piece.data)
    read += #piece.data
    
    while read - #pieces[1] > LOG_LIMIT do
      read -= #((table.remove(pieces, 1) ))
    end
  end
  local written, cut = text.tail(table.concat(pieces), LOG_LIMIT)
  local _, breaks = string.gsub(written, "\n", "")
  return { text = written, truncated = cut or breaks >= lines }, nil
end






function engine.watch(filters, changed)
  local reachable= nil
  while true do
    local choice = chosen
    local api = engine.reader()
    local events = if api and api:ping() then api:events { filters = filters } else nil
    if events then
      reachable = true
      watched[events] = true
      
      if choice ~= chosen then
        events:close()
      end
      
      changed()
      
      while events:next() do
        changed()
      end
      watched[events] = nil
      events:close()
      
      changed()
    elseif reachable ~= false then
      reachable = false
      changed()
    end
    
    if choice == chosen then
      task.wait(RETRY)
    end
  end
end

return engine
end function __DARKLUA_BUNDLE_MODULES.b()local v=__DARKLUA_BUNDLE_MODULES.cache.b if not v then v={c=__modImpl()}__DARKLUA_BUNDLE_MODULES.cache.b=v end return v.c end end do local function __modImpl()




local hash = require("lib:hash")
local hex = require("lib:hex")

local declaration = {}



local MADE = "1"

local MAX_WORDS = 4096
local MAX_PORTS = 256
local MAX_MOUNTS = 256










local function listed(list, limit)  
if type(list) ~= "table" or #list > limit then
    return false
  end
  local count = 0
  for _ in list do
    count += 1
  end
  return count == #list
end

local function words(list, limit)  
if list == nil then
    return true
  end
  if not listed(list, limit) then
    return false
  end
  for _, word in list do
    
    if type(word) ~= "string" or string.find(word, "\0", 1, true) then
      return false
    end
  end
  return true
end

local function portNumber(port)  
return type(port) == "number" and port == math.floor(port) and port >= 1 and port <= 65535
end


function declaration.refused(declared)  
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
  local environment= declared.environment or {}
  for _, variable in environment do
    if not string.match(variable, "^[^=]+=") then
      return "A container's environment is a list of NAME=value."
    end
  end
  local ports= declared.ports or {}
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
  local mounts= declared.mounts or {}
  if not listed(mounts, MAX_MOUNTS) then
    return "A container's folders are a list."
  end
  for _, mount in mounts do
    if
      type(mount) ~= "table"
      or type(mount.source) ~= "string"
      or type(mount.target) ~= "string"
      or string.find(mount.source .. mount.target, "\0", 1, true)
      
      or not (string.match(mount.source, "^/") or string.match(mount.source, "^%a:[/\\]"))
      or not string.match(mount.target, "^/")
    then
      return "A folder is given by its whole path, and a place in the container that starts with /."
    end
  end
  return nil
end


local function published(declared)  
local keys= {}
  local bindings= {}
  local ports= declared.ports or {}
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


function declaration.signature(declared)  
  
local parts= {}
  local function add(kind, values)
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
    local hosts= {}
    for _, binding in bindings[key] do
      table.insert(hosts, binding.HostPort)
    end
    add(`port {key}`, hosts)
  end
  local folders= declared.mounts or {}
  for _, mount in folders do
    add("mount", { mount.source, mount.target })
  end
  return hex.encode(hash.hash("sha256", table.concat(parts, "\n")))
end



function declaration.body(declared, labels)  
local _, bindings = published(declared)
  
  local mounts= {}
  local folders= declared.mounts or {}
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
      
      RestartPolicy = { Name = "unless-stopped" },
    },
  }
end

return declaration
end function __DARKLUA_BUNDLE_MODULES.c()local v=__DARKLUA_BUNDLE_MODULES.cache.c if not v then v={c=__modImpl()}__DARKLUA_BUNDLE_MODULES.cache.c=v end return v.c end end do local function __modImpl()












local app = require("vos:app")
local task = require("std:task")
local time = require("std:time")
local declaration = __DARKLUA_BUNDLE_MODULES.c()
local engine = __DARKLUA_BUNDLE_MODULES.b()

local owned = {}


local OWNER = "vos.app"
local SIGNATURE = "vos.signature"

local EVENTS = { "create", "start", "die", "destroy", "pause", "unpause", "restart" }


local BUSY_LIMIT = 180









function owned.container(name)  
local filter = { `{OWNER}={app.id}` }
  
  local problem = ""

  
  
  
  local since= nil
  
  local turn = 0

  
  local function list(api)    
local found, err = api:list_containers { all = true, filters = { label = filter } }
    if not found then
      return nil, engine.sentence(err)
    end
    local named= {}
    for _, summary in found do
      if summary.Names and table.find(summary.Names, `/{name}`) then
        table.insert(named, summary)
      end
    end
    return named, nil
  end

  local function removeAll(api)    
local found, unlisted = list(api)
    if not found then
      return unlisted
    end
    local failure= nil
    for _, summary in found do
      if summary.Id then
        local _, err = api:remove_container(summary.Id, { force = true })
        failure = failure or (if err then engine.sentence(err) else nil)
      end
    end
    return failure
  end

  
  local function pulled(api, image)    
local images, err = api:list_images { filters = { reference = { image } } }
    if not images then
      return engine.sentence(err)
    end
    if #images > 0 then
      return nil
    end
    local puller, unreachable = engine.puller()
    if not puller then
      return unreachable
    end
    local pull = puller:pull_image(image)
    
    while true do
      local step, step_err = pull:next()
      
      since = time.now()
      if step_err then
        return `The image {image} could not be pulled: {engine.sentence(step_err)}`
      end
      if not step then
        return nil
      end
    end
  end

  
  local function standing(api, signature)    
local found = list(api)
    local only = if found and #found == 1 then found[1] else nil
    return only ~= nil and only.State == "running" and (only.Labels or {})[SIGNATURE] == signature
  end

  local function make(api, declared, always)    
local wrong = declaration.refused(declared)
    if wrong then
      return wrong
    end
    local signature = declaration.signature(declared)
    if not always and standing(api, signature) then
      return nil
    end
    local missing = pulled(api, declared.image)
    if missing then
      return missing
    end
    local stale = removeAll(api)
    if stale then
      return stale
    end
    local body = declaration.body(declared, { [OWNER] = app.id, [SIGNATURE] = signature })
    local made, err = api:create_container(body, { name = name })
    if not made then
      return engine.sentence(err)
    end
    local _, start_err = api:start_container(made.Id)
    return if start_err then engine.sentence(start_err) else nil
  end

  local container = {}   
container.filters = { type = { "container" }, label = filter, event = EVENTS }

  
  
  function container.status(_self, declared)    
local status= { state = "absent", status = "", current = false, problem = problem }
    local api = engine.reader()
    local found = if api then list(api) else nil
    local first = if found then found[1] else nil
    if not first then
      return status
    end
    status.state = if first.State == "running"
      then "running"
      elseif first.State == "restarting" then "restarting"
      else "stopped"
    status.status = if type(first.Status) == "string" then first.Status else ""
    status.current = declared ~= nil
      and declaration.refused(declared) == nil
      and (first.Labels or {})[SIGNATURE] == declaration.signature(declared)
    return status
  end

  
  
  
  function container.apply(_self, declared, always)    
while since and since:elapsed():as_secs() < BUSY_LIMIT do
      task.wait(0.05)
    end
    turn += 1
    local mine = turn
    since = time.now()
    
    local api, unreachable = engine.actor()
    local ran, answer = true, unreachable
    if api then
      ran, answer = pcall(function()        
return if declared then make(api, declared, always == true) else removeAll(api)
      end)
    end
    local failure= if ran then answer else "The container could not be made."
    
    if turn == mine then
      since = nil
      
      problem = if declared then failure or "" else ""
    end
    return failure
  end

  
  function container.logs(_self, tail)    
local api, unreachable = engine.reader()
    if not api then
      return nil, unreachable
    end
    local found, unlisted = list(api)
    if not found then
      return nil, unlisted
    end
    local id = found[1] and found[1].Id
    if not id then
      return nil, nil
    end
    return engine.logs(id, tail)
  end

  return container
end

return owned
end function __DARKLUA_BUNDLE_MODULES.d()local v=__DARKLUA_BUNDLE_MODULES.cache.d if not v then v={c=__modImpl()}__DARKLUA_BUNDLE_MODULES.cache.d=v end return v.c end end end







local engine = __DARKLUA_BUNDLE_MODULES.b()
local owned = __DARKLUA_BUNDLE_MODULES.d()


local docker= {
  host = engine.host,
  use = engine.use,
  status = engine.status,
  sentence = engine.sentence,
  reader = engine.reader,
  actor = engine.actor,
  puller = engine.puller,
  logs = engine.logs,
  watch = engine.watch,
  container = owned.container,
}

return docker

-- GENERATED by scripts/codegen-lua.ts: crates/vos-lua/lua/docker/main.luau, bundled. Do not edit.
