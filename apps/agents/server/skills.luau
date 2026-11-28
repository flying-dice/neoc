--!strict
-- The skills a container agent uses, by name: the Skills app keeps them, and is asked over the bus (`list_skills` for
-- the names a form offers, `get_skill` for one skill's SKILL.md and other files when a chat's container is made). A
-- skill is a folder: copied into the container under the agent's skills folder, in a folder of its name
-- (sandbox.luau).
--
-- The Skills app's public requests are typed (vite.config.ts makes this app's types with it). Its paths are checked
-- here all the same: the archive a skill is packed in is unpacked at the container's root.
local skills = {}

-- A file of a skill besides its SKILL.md, by its path in the skill's folder.
export type File = { path: string, text: string }
-- A skill as it is copied into a container.
export type Skill = { name: string, text: string, files: { File } }

-- why the Skills app gave nothing, in the person's words
local function sentence(err: vos_rpc_RequestError?, what: string): string
  if err and err.kind == "not_found" then
    return "The Skills app is not there: install it, or open it, to use its skills."
  end
  return `The Skills app did not {what}: {if err then err.message else "it did not say why"}`
end

-- The skills of the Skills app, by name. When it is not there or does not answer, none, and why.
function skills.list(): agents_SkillChoice
  local listed: skills_ListSkillsResult?, err = RPC.send_request("skills", "list_skills")
  if not listed then
    return { skills = {}, problem = sentence(err, "list its skills") }
  end
  local out: { agents_SkillSummary } = {}
  for _, skill in listed.skills do
    table.insert(out, { name = skill.name, description = skill.description })
  end
  return { skills = out, problem = "" }
end

-- What is wrong with a path in a skill's folder; nil when nothing is. A path that leaves the folder is refused: the
-- archive is unpacked at the container's root.
local function badPath(path: string): string?
  if path == "" or string.sub(path, 1, 1) == "/" or string.find(path, "\\", 1, true) then
    return `{path} is not a path in the skill's folder.`
  end
  for part in string.gmatch(path .. "/", "([^/]*)/") do
    if part == "" or part == "." or part == ".." then
      return `{path} is not a path in the skill's folder.`
    end
  end
  return nil
end

-- The skill of that name, with its files. Otherwise why there is none, in the person's words.
function skills.get(name: string): (Skill?, string?)
  local got: skills_GetSkillResult?, err = RPC.send_request("skills", "get_skill", { name = name })
  if not got then
    return nil, sentence(err, `give the skill {name}`)
  end
  local files: { File } = {}
  for _, file in got.files do
    local wrong = badPath(file.path)
    if wrong then
      return nil, wrong
    end
    if file.path ~= "SKILL.md" then
      table.insert(files, { path = file.path, text = file.text })
    end
  end
  return { name = name, text = got.text, files = files }, nil
end

return skills
