--!strict
-- A copilot's image: built from its Dockerfile alone (the build context is that one file), tagged
-- `vos-copilot-<copilot id>:<the start of the Dockerfile's hash>` and labelled as this app's and the copilot's. So a
-- changed Dockerfile is a tag Docker does not have yet, and the same Dockerfile again is the image there is. A
-- copilot's `image` in the database is the tag its last build that succeeded made, and its `problem` why the last
-- build failed.
--
-- A build runs as a task of its own, one at a time per copilot, so that a turn that waits for it and is stopped does
-- not stop it half way. How it goes reaches the open windows as `building` notifications, a line at a time.
local docker = require("vos:docker")
local hash = require("lib:hash")
local hex = require("lib:hex")
local pack = require("./pack")
local task = require("std:task")
local time = require("std:time")
local db = app.db

-- how much of the Dockerfile's hash the tag carries, in hex digits
local HASH_LENGTH = 12
-- how often one that waits for a build looks whether it is over, in seconds
local WAIT = 0.5
-- how often the windows are told how a pull the build makes goes, at most, in milliseconds
local PROGRESS_EVERY = 1000
-- the labels every image of the app carries, with the copilot's id
local OWNER = "vos.app"
local COPILOT = "vos.copilot"

-- The copilots whose image is being built, with the tag the build makes.
local building: { [string]: string } = {}

local images = {}

-- The tag of the image of a copilot's Dockerfile.
function images.tag(id: string, dockerfile: string): string
  local digest = hex.encode(hash.hash("sha256", dockerfile))
  return `vos-copilot-{id}:{string.sub(digest, 1, HASH_LENGTH)}`
end

-- Whether the copilot's image is being built.
function images.building(id: string): boolean
  return building[id] ~= nil
end

local function tell(id: string, line: string, status: copilot_BuildStatus)
  RPC.send_notification("_building", { copilot = id, line = line, status = status })
end

-- Whether Docker has the image of that tag; nothing, and why, when Docker could not be asked.
local function has(tag: string): (boolean?, string?)
  local api, problem = docker.reader()
  if not api then
    return nil, problem
  end
  local found, err = api:list_images { filters = { reference = { tag } } }
  if not found then
    return nil, docker.sentence(err)
  end
  return #found > 0, nil
end

-- The build itself. Answers with why it failed; nothing when it made the image.
local function make(id: string, dockerfile: string, tag: string): string?
  local api, problem = docker.puller()
  if not api then
    return problem or "Docker cannot be reached."
  end
  local context, unpacked = pack.tar { { path = "Dockerfile", text = dockerfile } }
  if not context then
    return unpacked or "The Dockerfile could not be packed."
  end
  tell(id, `Building {tag}`, "running")
  local labels = { [OWNER] = app.id, [COPILOT] = id }
  local stream = api:build_image(context, { t = tag, labels = labels, rm = true, forcerm = true })
  local told: typeof(time.now())? = nil
  -- the build runs as its progress is read: a step that fails is the error of a `next()`
  while true do
    local step, err = stream:next()
    if err then
      return `The image could not be built: {docker.sentence(err)}`
    end
    if not step then
      break
    end
    if step.stream then
      for line in string.gmatch(step.stream, "[^\r\n]+") do
        tell(id, line, "running")
      end
    elseif step.status then
      -- a pull the build makes: each status is told, its progress (every few kilobytes) now and then
      local detail = step.progressDetail
      local moving = detail ~= nil and detail.total ~= nil
      if not moving or told == nil or told:elapsed():as_millis() >= PROGRESS_EVERY then
        told = if moving then time.now() else told
        tell(id, if step.id then `{step.id}: {step.status}` else step.status, "running")
      end
    end
  end
  local before = db:one("select image from copilots where id = :id", { id = id })
  db:exec("update copilots set image = :tag, problem = '' where id = :id", { tag = tag, id = id })
  -- the image of the Dockerfile before goes; Docker keeps it while a chat's container is made from it
  if before and before.image ~= "" and before.image ~= tag then
    api:remove_image(before.image)
  end
  tell(id, `Built {tag}.`, "done")
  return nil
end

-- Start a build of the copilot's image from its Dockerfile, and return at once. A build of it that is under way
-- goes on, and no second one starts.
function images.start(id: string, dockerfile: string)
  if building[id] then
    return
  end
  local tag = images.tag(id, dockerfile)
  building[id] = tag
  task.defer(function()
    local ok, failed = pcall(make, id, dockerfile, tag)
    local problem: string? = if ok then failed else `The build stopped: {tostring(failed)}`
    if problem then
      db:exec("update copilots set problem = :p where id = :id", { p = problem, id = id })
    end
    -- over only once what it made, or why it failed, is kept: one that waits for it reads that next
    building[id] = nil
    if problem then
      tell(id, problem, "failed")
    end
  end)
end

-- The image of the copilot's Dockerfile: the one Docker has, or one built now. Otherwise why there is none.
function images.ensure(id: string, dockerfile: string): (string?, string?)
  local tag = images.tag(id, dockerfile)
  while true do
    local present, problem = has(tag)
    if present == nil then
      return nil, problem
    elseif present then
      return tag, nil
    end
    -- a build of another Dockerfile of the copilot (it was changed since) is waited for, then this one is built
    images.start(id, dockerfile)
    local which = building[id]
    while building[id] do
      task.wait(WAIT)
    end
    if which == tag then
      local built = has(tag)
      if built then
        return tag, nil
      end
      local row = db:one("select problem from copilots where id = :id", { id = id })
      return nil, if row and row.problem ~= "" then row.problem else "The copilot's image was not built."
    end
  end
end

-- Remove every image of the copilot. Answers with why one could not be removed; nothing when they all were.
function images.remove(id: string): string?
  local api, problem = docker.actor()
  if not api then
    return problem
  end
  local found, err = api:list_images { filters = { label = { `{OWNER}={app.id}`, `{COPILOT}={id}` } } }
  if not found then
    return docker.sentence(err)
  end
  for _, image in found do
    local _, refused = api:remove_image(image.Id, { force = true })
    if refused and refused.kind ~= "not_found" then
      return `The copilot's image could not be removed: {docker.sentence(refused)}`
    end
  end
  return nil
end

return images
