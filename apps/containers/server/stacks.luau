--!strict
-- Compose stacks, read from the labels Compose puts on every container it creates. Docker itself knows nothing of
-- Compose: a stack is the containers that carry the same project label, and this is where those labels are read.
-- Nothing here talks to Docker.
local rpc = require("../.app/rpc")

local stacks = {}

local PROJECT = "com.docker.compose.project"
local SERVICE = "com.docker.compose.service"
local FOLDER = "com.docker.compose.project.working_dir"
-- `db:service_started:false,cache:service_healthy:true`: the services this one waits for
local DEPENDS_ON = "com.docker.compose.depends_on"

export type Labels = { [string]: string }

-- a container of a stack, as far as the order to start and stop it in goes
export type Member = { id: string, name: string, service: string, needs: { string } }

-- the stack and the service a container belongs to: both empty for a container on its own
function stacks.of(labels: Labels): (string, string)
  local stack = labels[PROJECT] or ""
  return stack, if stack == "" then "" else labels[SERVICE] or ""
end

-- the folder a stack was brought up from, when Compose recorded it
function stacks.folder(labels: Labels): string
  return labels[FOLDER] or ""
end

-- Docker's filter for what belongs to a stack: its containers, and its networks
function stacks.filter(stack: string): { [string]: { string } }
  return { label = { `{PROJECT}={stack}` } }
end

-- the services a container waits for
function stacks.needs(labels: Labels): { string }
  local needs: { string } = {}
  for entry in string.gmatch(labels[DEPENDS_ON] or "", "[^,]+") do
    local service = string.match(entry, "^%s*([^:%s]+)")
    if service then
      table.insert(needs, service)
    end
  end
  return needs
end

-- the stacks a list of containers belongs to, by name; `folders` is the folder of each, where one is known
function stacks.summarise(containers: { rpc.Container }, folders: { [string]: string }): { rpc.Stack }
  local byName: { [string]: rpc.Stack } = {}
  local list: { rpc.Stack } = {}
  for _, container in containers do
    if container.stack ~= "" then
      local stack: rpc.Stack = byName[container.stack]
        or { name = container.stack, running = 0, total = 0, folder = folders[container.stack] or "" }
      if not byName[container.stack] then
        byName[container.stack] = stack
        table.insert(list, stack)
      end
      stack.total += 1
      if container.state == "running" then
        stack.running += 1
      end
    end
  end
  table.sort(list, function(a, b)
    return a.name < b.name
  end)
  return list
end

-- The members in the order to start them: a service after the ones it waits for, by name otherwise. Stopping is the
-- reverse. A service that waits for one the stack does not have (or a circle of them) is not held back for ever: what
-- is left when nothing more can be placed follows by name.
function stacks.order(members: { Member }): { Member }
  local left = table.clone(members)
  table.sort(left, function(a, b)
    return a.name < b.name
  end)
  local present: { [string]: boolean } = {}
  for _, member in left do
    present[member.service] = true
  end
  local placed: { [string]: boolean } = {}
  local ordered: { Member } = {}

  local function ready(member: Member): boolean
    for _, service in member.needs do
      if present[service] and not placed[service] and service ~= member.service then
        return false
      end
    end
    return true
  end

  while #left > 0 do
    local waiting: { Member } = {}
    local started: { Member } = {}
    for _, member in left do
      table.insert(if ready(member) then started else waiting, member)
    end
    if #started == 0 then
      -- a circle: the rest as it is
      table.move(waiting, 1, #waiting, #ordered + 1, ordered)
      break
    end
    for _, member in started do
      table.insert(ordered, member)
    end
    -- a service counts as started once every container of it in this round is placed
    for _, member in started do
      placed[member.service] = true
    end
    left = waiting
  end
  return ordered
end

return stacks
