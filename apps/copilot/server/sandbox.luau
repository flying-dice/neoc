--!strict
-- A chat's container, `vos-copilot-<chat id>`: made from its copilot's image the first time the chat takes a turn,
-- with a volume of its own of the same name mounted at the copilot's work folder, and kept until the chat is removed.
-- The agent is the container's program (the copilot's command) and speaks ACP on its standard streams, which
-- link.luau attaches to. The container runs while its chat is connected and is stopped otherwise; what the agent
-- keeps (its sessions, under its home, and the work folder) stays with it.
--
-- It is made with the copilot as it is then: the image of its Dockerfile, its command, its folders, its environment
-- (a keychain password is read now and never kept) and its skills, copied into the skills folder before it first
-- starts. A change to the copilot reaches the chats started after it. The container and the volume carry labels that
-- say they are this app's, the copilot's and the chat's.
local copilots = require("./copilots")
local docker = require("vos:docker")
local images = require("./images")
local pack = require("./pack")
local skills = require("./skills")
local task = require("std:task")
local db = app.db

-- Seconds an agent is given to end when its container is stopped, before Docker kills it.
local STOP_TIMEOUT = 5
-- how often one that waits for a container to stop looks whether it has, in seconds
local WAIT = 0.2
-- where this machine is, from inside a container
local HOST_GATEWAY = "host.docker.internal:host-gateway"

-- the chats whose container is being stopped
local stopping: { [string]: boolean } = {}

local sandbox = {}

-- The name of a chat's container, and of its volume.
function sandbox.name(chat: string): string
  return `vos-copilot-{chat}`
end

-- wait while the chat's container is being stopped: what is asked of it next comes after
local function settled(chat: string)
  while stopping[chat] do
    task.wait(WAIT)
  end
end

-- The copilot's skills, as an archive to unpack at the container's root: each in a folder of its name under the
-- skills folder. Neither an archive nor a problem: it uses none.
local function skillsArchive(copilot: copilots.Copilot): (string?, string?)
  local chosen = skills.of(copilot.skills)
  if #chosen == 0 then
    return nil, nil
  end
  local base = string.match(copilot.skills_dir, "^/*(.-)/*$") or ""
  local files: { pack.File } = {}
  for _, skill in chosen do
    local folder = if base == "" then skill.name else `{base}/{skill.name}`
    table.insert(files, { path = `{folder}/SKILL.md`, text = skill.text })
    for _, file in skill.files do
      table.insert(files, { path = `{folder}/{file.path}`, text = file.text })
    end
  end
  local archive, problem = pack.tar(files)
  if not archive then
    return nil, `The skills could not be packed: {problem or "it did not say why"}`
  end
  return archive, nil
end

-- Make the chat's volume and container, and copy the copilot's skills into it. Answers with why it could not.
local function make(api: vnd_bollard_Client, chat: string, copilot: copilots.Copilot): string?
  local image, unbuilt = images.ensure(copilot.id, copilot.dockerfile)
  if not image then
    return unbuilt
  end
  local env, unresolved = copilots.resolve(copilot.env)
  if not env then
    return unresolved
  end
  local archive, unpacked = skillsArchive(copilot)
  if unpacked then
    return unpacked
  end
  local name = sandbox.name(chat)
  local labels = { ["vos.app"] = app.id, ["vos.copilot"] = copilot.id, ["vos.thread"] = chat }
  local _, unmade = api:create_volume { Name = name, Labels = labels }
  if unmade then
    return docker.sentence(unmade)
  end
  -- from here on the chat has something in Docker to remove with it; the folder its sessions start in is the one
  -- its volume is mounted at
  db:exec("update threads set sandbox = 1, workdir = :w where id = :id", { w = copilot.workdir, id = chat })
  local variables: { string } = {}
  for _, variable in env do
    table.insert(variables, `{variable.name}={variable.value}`)
  end
  local volume: vnd_bollard_Mount = { Type = "volume", Source = name, Target = copilot.workdir }
  local made, refused = api:create_container({
    Image = image,
    Cmd = copilot.command,
    WorkingDir = copilot.workdir,
    Env = variables,
    Labels = labels,
    -- the agent's standard streams are the connection: its input stays open from one attach to the next
    OpenStdin = true,
    StdinOnce = false,
    AttachStdin = true,
    AttachStdout = true,
    AttachStderr = true,
    Tty = false,
    HostConfig = {
      Mounts = { volume },
      ExtraHosts = { HOST_GATEWAY },
    },
  }, { name = name })
  if not made then
    return docker.sentence(refused)
  end
  if archive then
    local _, failed = api:upload_to_container(made.Id, { path = "/" }, archive)
    if failed then
      api:remove_container(made.Id, { force = true })
      return `The skills could not be copied into the container: {docker.sentence(failed)}`
    end
  end
  return nil
end

-- Have the chat's container, stopped, for link.luau to attach to and start: made when the chat has none, and stopped
-- when it runs (the agent in it was spoken to on a connection that is gone, and starts afresh). Answers with the
-- folder its sessions start in, or why there is no container.
function sandbox.ensure(chat: string, copilot: copilots.Copilot): (string?, string?)
  settled(chat)
  local api, unreachable = docker.actor()
  if not api then
    return nil, unreachable
  end
  local name = sandbox.name(chat)
  local found, err = api:inspect_container(name)
  if err then
    return nil, docker.sentence(err)
  end
  if found then
    local labels: { [string]: string } = (found.Config and found.Config.Labels) or {}
    if labels["vos.app"] ~= app.id or labels["vos.thread"] ~= chat then
      return nil, `A container named {name} that is not this chat's is in the way.`
    end
    if found.State and found.State.Running then
      local _, refused = api:stop_container(name, { timeout = STOP_TIMEOUT })
      if refused then
        return nil, docker.sentence(refused)
      end
    end
  else
    local failed = make(api, chat, copilot)
    if failed then
      return nil, failed
    end
  end
  local row = db:one("select workdir from threads where id = :id", { id = chat })
  return if row and row.workdir ~= "" then row.workdir else copilot.workdir, nil
end

-- Start the chat's container. Answers with why it could not.
function sandbox.start(chat: string): string?
  local api, unreachable = docker.actor()
  if not api then
    return unreachable
  end
  local _, refused = api:start_container(sandbox.name(chat))
  return if refused then `The copilot's container did not start: {docker.sentence(refused)}` else nil
end

-- Stop the chat's container, as it is idle or its connection has gone. Whatever Docker says is left: the next turn
-- stops a container that still runs (`ensure`).
function sandbox.stop(chat: string)
  stopping[chat] = true
  pcall(function()
    local api = docker.actor()
    if api then
      api:stop_container(sandbox.name(chat), { timeout = STOP_TIMEOUT })
    end
  end)
  stopping[chat] = nil
end

-- Remove the chat's container and volume. Answers with why one could not be removed; one that is not there is
-- removed already.
function sandbox.remove(chat: string): string?
  settled(chat)
  local api, unreachable = docker.actor()
  if not api then
    return unreachable
  end
  local name = sandbox.name(chat)
  local _, refused = api:remove_container(name, { force = true })
  if refused and refused.kind ~= "not_found" then
    return `The chat's container could not be removed: {docker.sentence(refused)}`
  end
  local _, kept = api:remove_volume(name)
  if kept and kept.kind ~= "not_found" then
    return `The chat's volume could not be removed: {docker.sentence(kept)}`
  end
  return nil
end

return sandbox
