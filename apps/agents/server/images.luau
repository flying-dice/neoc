--!strict
-- The images Docker has, which a container agent's chats are made from. The Containers app builds and pulls them; this
-- app only lists them, for a form to choose one from, and looks one up before a chat's container is made from it.
local docker = require("vos:docker")

local images = {}

-- The tag an image's name stands for: Docker's `latest` when it names none (the part after the last `/` has no `:`).
-- A name with a digest (`@sha256:...`) is that image itself.
function images.tagOf(name: string): string
  if string.find(name, "@", 1, true) then
    return name
  end
  local last = string.match(name, "([^/]*)$") or name
  return if string.find(last, ":", 1, true) then name else `{name}:latest`
end

-- The images Docker has, each by name and tag, in name order. When Docker cannot be reached, none, and why.
function images.list(): agents_ImageChoice
  local api, unreachable = docker.reader()
  if not api then
    return { images = {}, problem = unreachable or "Docker cannot be reached." }
  end
  local found, err = api:list_images()
  if not found then
    return { images = {}, problem = docker.sentence(err) }
  end
  local seen: { [string]: boolean } = {}
  local out: { string } = {}
  for _, image in found do
    local tags: { string } = image.RepoTags or {}
    for _, tag in tags do
      -- an image that lost its name to a newer one has none
      if tag ~= "<none>:<none>" and not seen[tag] then
        seen[tag] = true
        table.insert(out, tag)
      end
    end
  end
  table.sort(out)
  return { images = out, problem = "" }
end

-- Whether Docker has the image of that name. Nothing, and why, when Docker could not be asked.
function images.present(name: string): (boolean?, string?)
  local api, unreachable = docker.reader()
  if not api then
    return nil, unreachable
  end
  local found, err = api:list_images { filters = { reference = { name } } }
  if not found then
    return nil, docker.sentence(err)
  end
  local wanted = images.tagOf(name)
  for _, image in found do
    if wanted == name and string.find(name, "@", 1, true) then
      return true, nil
    end
    local tags: { string } = image.RepoTags or {}
    for _, tag in tags do
      if tag == wanted or tag == name then
        return true, nil
      end
    end
  end
  return false, nil
end

return images
