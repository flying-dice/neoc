--!strict
-- The skills a container agent uses, by name: the Skills app keeps them, and is asked over the bus (`list_skills` for
-- the names a form offers, `get_skill` for one skill's SKILL.md and other files when a chat's container is made). A
-- skill is a folder: copied into the container under the agent's skills folder, in a folder of its name
-- (sandbox.luau).
--
-- The Skills app is not one this app's types are made with, so what it answers is checked here, as text from
-- anywhere would be.
local skills = {}

-- A file of a skill besides its SKILL.md, by its path in the skill's folder.
export type File = { path: string, text: string }
-- A skill as it is copied into a container.
export type Skill = { name: string, text: string, files: { File } }

-- A request to the Skills app: its answer, or why there is none.
local function ask(method: string, params: { [string]: any }?): (any, vos_rpc_RequestError?)
  local bus: any = RPC
  return bus.send_request("skills", method, params)
end

-- why the Skills app gave nothing, in the person's words
local function sentence(err: vos_rpc_RequestError?, what: string): string
  if err and err.kind == "not_found" then
    return "The Skills app is not there: install it, or open it, to use its skills."
  end
  return `The Skills app did not {what}: {if err then err.message else "it did not say why"}`
end

-- The skills of the Skills app, by name. When it is not there or does not answer, none, and why.
function skills.list(): agents_SkillChoice
  local listed, err = ask("list_skills", nil)
  if type(listed) ~= "table" then
    return { skills = {}, problem = sentence(err, "list its skills") }
  end
  local out: { agents_SkillSummary } = {}
  for _, skill in if type(listed.skills) == "table" then listed.skills else {} do
    if type(skill) == "table" and type(skill.name) == "string" then
      table.insert(out, {
        name = skill.name,
        description = if type(skill.description) == "string" then skill.description else "",
      })
    end
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
  local got, err = ask("get_skill", { name = name })
  if type(got) ~= "table" then
    return nil, sentence(err, `give the skill {name}`)
  end
  if type(got.text) ~= "string" then
    return nil, `The Skills app gave the skill {name} without its SKILL.md.`
  end
  local files: { File } = {}
  for _, file in if type(got.files) == "table" then got.files else {} do
    if type(file) ~= "table" or type(file.path) ~= "string" or type(file.text) ~= "string" then
      return nil, `The Skills app gave a file of the skill {name} without its path or its text.`
    end
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
