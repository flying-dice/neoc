--!strict
-- Docker's images, volumes and networks in the app's own shapes, each with the containers that use it, and what a
-- person makes and removes of them. What a volume or a network is made with is checked here before Docker hears of
-- it, and said in a sentence when it is wrong. Which Docker, its clients and why a call failed are the container
-- framework's (`vos:docker`); pulling an image is the engine's, as a container's image is pulled there too.
--
-- A function that does something answers with a plain sentence when it could not, and with nothing when it did.
local docker = require("vos:docker")
local spec = require("./spec")

local objects = {}

local sentence = docker.sentence
local trim = spec.trim

-- Docker's own networks: every Docker has them, and none can be removed
local PREDEFINED: { [string]: boolean } = { bridge = true, host = true, none = true }
-- what Docker lists for an image that has no name
local UNNAMED = "<none>:<none>"

-- What of Docker's the containers use, each by the names of the containers that use it (running or not), in order;
-- `running` has only the running ones on each network.
type Users = {
  images: { [string]: { string } },
  volumes: { [string]: { string } },
  networks: { [string]: { string } },
  running: { [string]: { string } },
}

local function name_of(names: { string }?): string
  local first = if names then names[1] else nil
  return if first then (string.gsub(first, "^/", "")) else ""
end

local function add(map: { [string]: { string } }, key: string, name: string)
  local list = map[key] or {}
  map[key] = list
  table.insert(list, name)
end

-- names as a sentence says them: `a`, `a and b`, `a, b and c`
local function said(names: { string }): string
  if #names <= 1 then
    return names[1] or ""
  end
  return `{table.concat(names, ", ", 1, #names - 1)} and {names[#names]}`
end

local function users(api: vnd_bollard_Client): (Users?, string?)
  local list, err = api:list_containers { all = true }
  if not list then
    return nil, sentence(err)
  end
  local found: Users = { images = {}, volumes = {}, networks = {}, running = {} }
  for _, summary in list do
    local name = name_of(summary.Names)
    if summary.ImageID then
      add(found.images, summary.ImageID, name)
    end
    local mounts: { vnd_bollard_MountPoint } = summary.Mounts or {}
    for _, mount in mounts do
      if mount.Type == "volume" and mount.Name then
        add(found.volumes, mount.Name, name)
      end
    end
    local joined: { [string]: vnd_bollard_EndpointSettings } = (
      summary.NetworkSettings and summary.NetworkSettings.Networks
    ) or {}
    for network in joined do
      add(found.networks, network, name)
      if summary.State == "running" then
        add(found.running, network, name)
      end
    end
  end
  for _, map in { found.images, found.volumes, found.networks, found.running } do
    for _, names in map do
      table.sort(names)
    end
  end
  return found, nil
end

-- the labels a form gives, as Docker takes them
local function labels_of(entries: { containers_Entry }): { [string]: string }
  local labels: { [string]: string } = {}
  for _, entry in entries do
    labels[trim(entry.name)] = entry.value
  end
  return labels
end

-- a driver's name: a word, or a plugin's `owner/name:tag`
local function is_driver(driver: string): boolean
  return string.match(driver, "^[%w][%w_%.%-/:]*$") ~= nil
end

-- IPv4's four numbers and the size of the prefix, or IPv6's hex and colons and the size of its prefix
local function is_subnet(subnet: string): boolean
  local address, size = string.match(subnet, "^([^/]+)/(%d+)$")
  if not address or not spec.isAddress(address) then
    return false
  end
  local bits = tonumber(size) :: number
  return bits <= (if string.find(address, ":", 1, true) then 128 else 32)
end

-- Images

-- The images Docker has: those with a name first, by name, then the dangling ones, newest first.
function objects.images(): ({ containers_Image }?, string?)
  local api, problem = docker.reader()
  if not api then
    return nil, problem
  end
  local used, used_err = users(api)
  if not used then
    return nil, used_err
  end
  local list, err = api:list_images()
  if not list then
    return nil, sentence(err)
  end
  local images: { containers_Image } = {}
  for _, summary in list do
    local tags: { string } = {}
    local named: { string } = summary.RepoTags or {}
    for _, tag in named do
      if tag ~= UNNAMED then
        table.insert(tags, tag)
      end
    end
    table.sort(tags)
    table.insert(images, {
      id = summary.Id,
      tags = tags,
      size = summary.Size,
      created = summary.Created,
      containers = used.images[summary.Id] or {},
    })
  end
  table.sort(images, function(a, b)
    local x, y = a.tags[1], b.tags[1]
    if x and y then
      return if x == y then a.id < b.id else x < y
    elseif x or y then
      return x ~= nil
    end
    return if a.created == b.created then a.id < b.id else a.created > b.created
  end)
  return images, nil
end

-- Remove an image, by its id, with all its names. Docker removes one a container was made from only when `force`,
-- and one a running container was made from not even then: what it says is passed on.
function objects.remove_image(image: string, force: boolean): string?
  if not string.match(image, "^sha256:%x+$") and not spec.isImage(image) then
    return "There is no such image."
  end
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  local _, err = api:remove_image(image, { force = force })
  return if err then sentence(err) else nil
end

-- Remove the dangling images: those with no name, that no container was made from. Answers with how many went and
-- how many bytes that freed.
function objects.prune_images(): (number?, number?, string?)
  local api, problem = docker.actor()
  if not api then
    return nil, nil, problem
  end
  local before, list_err = api:list_images()
  if not before then
    return nil, nil, sentence(list_err)
  end
  local there: { [string]: boolean } = {}
  for _, summary in before do
    there[summary.Id] = true
  end
  local pruned, err = api:prune_images()
  if not pruned then
    return nil, nil, sentence(err)
  end
  -- Docker lists the layers it deleted beside the images: the images are those that were listed
  local removed = 0
  local deleted: { vnd_bollard_ImageDeleteResponseItem } = pruned.ImagesDeleted or {}
  for _, item in deleted do
    if item.Deleted and there[item.Deleted] then
      removed += 1
    end
  end
  return removed, pruned.SpaceReclaimed or 0, nil
end

-- Volumes

-- Docker's volumes, by name.
function objects.volumes(): ({ containers_Volume }?, string?)
  local api, problem = docker.reader()
  if not api then
    return nil, problem
  end
  local used, used_err = users(api)
  if not used then
    return nil, used_err
  end
  local list, err = api:list_volumes()
  if not list then
    return nil, sentence(err)
  end
  local volumes: { containers_Volume } = {}
  for _, volume in list.Volumes do
    table.insert(volumes, {
      name = volume.Name,
      driver = volume.Driver,
      mountpoint = volume.Mountpoint,
      created = volume.CreatedAt or "",
      containers = used.volumes[volume.Name] or {},
    })
  end
  table.sort(volumes, function(a, b)
    return a.name < b.name
  end)
  return volumes, nil
end

-- Make a volume: by the name given (empty: Docker makes one up), kept by the driver given (empty: `local`), with
-- the labels given. Docker answers a name it has with the volume it has: that is said instead.
function objects.create_volume(name: string, driver: string, labels: { containers_Entry }): string?
  local wanted = trim(name)
  if wanted ~= "" and not spec.isName(wanted) then
    return "A volume's name starts with a letter or a digit, and has only letters, digits, _, . and -."
  end
  local keeper = if trim(driver) == "" then "local" else trim(driver)
  if not is_driver(keeper) then
    return `{keeper} is not the name of a driver: Docker's own is local.`
  end
  local wrong = spec.entries(labels, "Label", "labels")
  if wrong then
    return wrong
  end
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  if wanted ~= "" then
    -- Docker's filter by name matches a part of it: the volume is the one named exactly so
    local same, list_err = api:list_volumes { filters = { name = { wanted } } }
    if not same then
      return sentence(list_err)
    end
    for _, volume in same.Volumes do
      if volume.Name == wanted then
        return `There is a volume named {wanted} already.`
      end
    end
  end
  local _, err = api:create_volume {
    Name = if wanted == "" then nil else wanted,
    Driver = keeper,
    Labels = labels_of(labels),
  }
  return if err then sentence(err) else nil
end

-- Remove a volume and its data. Docker removes one only when no container uses it, running or not: the containers
-- that do are named instead.
function objects.remove_volume(name: string): string?
  if not spec.isName(name) then
    return "There is no such volume."
  end
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  local used, used_err = users(api)
  if not used then
    return used_err
  end
  local by = used.volumes[name]
  if by and #by > 0 then
    return `The volume {name} is used by {said(by)}: Docker removes a volume only once no container uses it, running or not.`
  end
  local _, err = api:remove_volume(name)
  return if err then sentence(err) else nil
end

-- Remove every volume no container uses, named or not, and the data in them. Answers with the names of those that
-- went and how many bytes that freed.
function objects.prune_volumes(): ({ string }?, number?, string?)
  local api, problem = docker.actor()
  if not api then
    return nil, nil, problem
  end
  -- since Docker 23 a prune takes only the anonymous volumes unless it is asked for all of them
  local pruned, err = api:prune_volumes { filters = { all = { "true" } } }
  if not pruned then
    return nil, nil, sentence(err)
  end
  local removed: { string } = pruned.VolumesDeleted or {}
  table.sort(removed)
  return removed, pruned.SpaceReclaimed or 0, nil
end

-- Networks

-- Docker's networks, by name.
function objects.networks(): ({ containers_Network }?, string?)
  local api, problem = docker.reader()
  if not api then
    return nil, problem
  end
  local used, used_err = users(api)
  if not used then
    return nil, used_err
  end
  local list, err = api:list_networks()
  if not list then
    return nil, sentence(err)
  end
  local networks: { containers_Network } = {}
  for _, network in list do
    local name = network.Name
    if name then
      local subnets: { string } = {}
      local gateways: { string } = {}
      local configs: { vnd_bollard_IpamConfig } = (network.IPAM and network.IPAM.Config) or {}
      for _, config in configs do
        if config.Subnet and config.Subnet ~= "" then
          table.insert(subnets, config.Subnet)
        end
        if config.Gateway and config.Gateway ~= "" then
          table.insert(gateways, config.Gateway)
        end
      end
      table.insert(networks, {
        name = name,
        driver = network.Driver or "",
        scope = network.Scope or "",
        subnet = table.concat(subnets, ", "),
        gateway = table.concat(gateways, ", "),
        internal = network.Internal == true,
        attachable = network.Attachable == true,
        predefined = PREDEFINED[name] == true,
        containers = used.networks[name] or {},
      })
    end
  end
  table.sort(networks, function(a, b)
    return a.name < b.name
  end)
  return networks, nil
end

-- What a network is made with, as the form has it.
export type NetworkAsked = {
  name: string,
  driver: string,
  subnet: string,
  gateway: string,
  internal: boolean,
  attachable: boolean,
  labels: { containers_Entry },
}

-- Make a network: by its name, with the driver given (empty: `bridge`), and the subnet and gateway given (empty:
-- Docker chooses them). A name another network has is Docker's to refuse.
function objects.create_network(asked: NetworkAsked): string?
  local name = trim(asked.name)
  if name == "" then
    return "Say what the network is called."
  end
  if not spec.isName(name) then
    return "A network's name starts with a letter or a digit, and has only letters, digits, _, . and -."
  end
  local driver = if trim(asked.driver) == "" then "bridge" else trim(asked.driver)
  if not is_driver(driver) then
    return `{driver} is not the name of a driver, like bridge, overlay or macvlan.`
  end
  local subnet, gateway = trim(asked.subnet), trim(asked.gateway)
  if subnet ~= "" and not is_subnet(subnet) then
    return `{subnet} is not a subnet. A subnet is an address and the size of its prefix, like 172.30.0.0/16.`
  end
  if gateway ~= "" then
    if subnet == "" then
      return "A gateway is an address in the network's subnet: say the subnet too."
    end
    if not spec.isAddress(gateway) then
      return `{gateway} is not an address, like 172.30.0.1.`
    end
  end
  local wrong = spec.entries(asked.labels, "Label", "labels")
  if wrong then
    return wrong
  end
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  local _, err = api:create_network {
    Name = name,
    Driver = driver,
    Internal = asked.internal,
    Attachable = asked.attachable,
    Labels = labels_of(asked.labels),
    IPAM = if subnet == ""
      then nil
      else { Config = { { Subnet = subnet, Gateway = if gateway == "" then nil else gateway } } },
  }
  return if err then sentence(err) else nil
end

-- Remove a network. Docker's own cannot be removed, and Docker removes one only when no running container is on
-- it: those that are are named instead.
function objects.remove_network(name: string): string?
  if PREDEFINED[name] then
    return `{name} is Docker's own network: it cannot be removed.`
  end
  if not spec.isName(name) then
    return "There is no such network."
  end
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  local used, used_err = users(api)
  if not used then
    return used_err
  end
  local on = used.running[name]
  if on and #on > 0 then
    return `{said(on)} {if #on == 1 then "runs" else "run"} on the network {name}: Docker removes a network only once no running container is on it.`
  end
  local _, err = api:remove_network(name)
  return if err then sentence(err) else nil
end

return objects
