--!strict
-- A container agent's chat's container, `vos-agent-<chat id>`: made from the agent's image the first time the chat
-- takes a turn, with a volume of its own of the same name mounted at the agent's work folder, and kept until the chat
-- is removed. The agent is the container's program (the agent's command) and speaks ACP on its standard streams,
-- which transport.luau attaches to. The container runs while its chat is connected and is stopped otherwise; what the
-- agent keeps (its sessions, under its home, and the work folder) stays with it.
--
-- It is made with the agent as it is then: its image (which the Containers app built or pulled), its command, its
-- folders, its environment (a keychain password is read now and never kept) and its skills, fetched from the Skills
-- app and copied into the skills folder before it first starts. A change to the agent reaches the chats started after
-- it. The container and the volume carry labels that say they are this app's, the agent's and the chat's.
local docker = require("vos:docker")
local images = require("./images")
local pack = require("./pack")
local roster = require("./roster")
local skills = require("./skills")
local task = require("std:task")
local timeline = require("./timeline")
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
  return `vos-agent-{chat}`
end

-- wait while the chat's container is being stopped: what is asked of it next comes after
local function settled(chat: string)
  while stopping[chat] do
    task.wait(WAIT)
  end
end

-- The agent's skills, as an archive to unpack at the container's root: each in a folder of its name under the skills
-- folder. A skill the Skills app does not give (it was removed, say) is left out, with a note in the chat. Neither an
-- archive nor a problem: there is none to copy.
local function skillsArchive(chat: string, agent: roster.Agent): (string?, string?)
  local base = string.match(agent.skills_dir, "^/*(.-)/*$") or ""
  local files: { pack.File } = {}
  for _, name in agent.skills do
    local skill, missing = skills.get(name)
    if skill then
      local folder = if base == "" then skill.name else `{base}/{skill.name}`
      table.insert(files, { path = `{folder}/SKILL.md`, text = skill.text })
      for _, file in skill.files do
        table.insert(files, { path = `{folder}/{file.path}`, text = file.text })
      end
    else
      timeline.add(chat, "note", {
        text = `The skill {name} was not copied into this chat's container: {missing or "the Skills app did not give it"}`,
      })
    end
  end
  if #files == 0 then
    return nil, nil
  end
  local archive, problem = pack.tar(files)
  if not archive then
    return nil, `The skills could not be packed: {problem or "it did not say why"}`
  end
  return archive, nil
end

-- Make the chat's volume and container, and copy the agent's skills into it. Answers with why it could not.
local function make(api: vnd_bollard_Client, chat: string, agent: roster.Agent): string?
  local present, unasked = images.present(agent.image)
  if present == nil then
    return unasked
  end
  if not present then
    return `Docker has no image {agent.image}. Build or pull {agent.image} in Containers first.`
  end
  local env, unresolved = roster.resolve(agent.env)
  if not env then
    return unresolved
  end
  local archive, unpacked = skillsArchive(chat, agent)
  if unpacked then
    return unpacked
  end
  local name = sandbox.name(chat)
  local labels = { ["vos.app"] = app.id, ["vos.agent"] = agent.id, ["vos.thread"] = chat }
  local _, unmade = api:create_volume { Name = name, Labels = labels }
  if unmade then
    return docker.sentence(unmade)
  end
  -- from here on the chat has something in Docker to remove with it; the folder its sessions start in is the one
  -- its volume is mounted at
  db:exec("update threads set sandbox = 1, workdir = :w where id = :id", { w = agent.folder, id = chat })
  local variables: { string } = {}
  for _, variable in env do
    table.insert(variables, `{variable.name}={variable.value}`)
  end
  local volume: vnd_bollard_Mount = { Type = "volume", Source = name, Target = agent.folder }
  local made, refused = api:create_container({
    Image = agent.image,
    Cmd = agent.command,
    WorkingDir = agent.folder,
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

-- Have the chat's container, stopped, for transport.luau to attach to and start: made when the chat has none, and
-- stopped when it runs (the agent in it was spoken to on a connection that is gone, and starts afresh). Answers with
-- the folder its sessions start in, or why there is no container.
function sandbox.ensure(chat: string, agent: roster.Agent): (string?, string?)
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
    local failed = make(api, chat, agent)
    if failed then
      return nil, failed
    end
  end
  local row = db:one("select workdir from threads where id = :id", { id = chat })
  return if row and row.workdir ~= "" then row.workdir else agent.folder, nil
end

-- Start the chat's container. Answers with why it could not.
function sandbox.start(chat: string): string?
  local api, unreachable = docker.actor()
  if not api then
    return unreachable
  end
  local _, refused = api:start_container(sandbox.name(chat))
  return if refused then `The agent's container did not start: {docker.sentence(refused)}` else nil
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
