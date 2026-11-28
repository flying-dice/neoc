--!strict
-- The skills, in the app's database: each a name, its `SKILL.md` and the other files of its folder, by their paths
-- there. An app that runs agents lists them and asks for one by its name (main.luau), and copies it where its agent
-- reads it, in a folder of its name. A copy given before keeps what it was given: a change reaches the copies made
-- after it.
local codec = require("./codec")
local random = require("lib:random")
local strings = require("lib:text")
local db = app.db

export type File = skills_SkillFile
export type Skill = skills_Skill

local skills = {}

local COLUMNS = "id, name, text, files"

-- A YAML scalar as it is written on one line: without the quotes around it, and with the comment after a plain one
-- taken off.
local function scalar(value: string): string
  value = strings.trim(value)
  local first = string.sub(value, 1, 1)
  if first == '"' then
    local inner = string.match(value, '^"(.*)"$')
    if inner then
      return (
        string.gsub(inner, "\\(.)", function(c: string): string
          return if c == "n" then "\n" elseif c == "t" then "\t" else c
        end)
      )
    end
  elseif first == "'" then
    local inner = string.match(value, "^'(.*)'$")
    if inner then
      return (string.gsub(inner, "''", "'"))
    end
  end
  return strings.trim((string.gsub(value, "%s+#.*$", "")))
end

-- What a skill is for: the `description` of its SKILL.md's front matter (the YAML between the `---` lines it starts
-- with), as one line. Empty when it has none. A folded or literal value (`>` or `|`) is its indented lines after it,
-- joined by spaces.
function skills.description(text: string): string
  local lines: { string } = {}
  for line in string.gmatch((string.gsub(text, "\r\n", "\n")) .. "\n", "([^\n]*)\n") do
    table.insert(lines, line)
  end
  if strings.trim(lines[1] or "") ~= "---" then
    return ""
  end
  local i = 2
  while i <= #lines and strings.trim(lines[i]) ~= "---" do
    local value = string.match(lines[i], "^description%s*:(.*)$")
    if value then
      value = strings.trim(value)
      if string.match(value, "^[>|][+-]?$") then
        local parts: { string } = {}
        i += 1
        while i <= #lines and (string.match(lines[i], "^%s") or strings.trim(lines[i]) == "") do
          if strings.trim(lines[i]) ~= "" then
            table.insert(parts, strings.trim(lines[i]))
          end
          i += 1
        end
        return table.concat(parts, " ")
      end
      return scalar(value)
    end
    i += 1
  end
  -- front matter that is never closed is not front matter
  return ""
end

local function skillOf(row: { [string]: any }): Skill
  local files: { File } = {}
  for _, file in codec.decode(row.files) or {} do
    table.insert(files, { path = file.path, text = file.text })
  end
  return { id = row.id, name = row.name, description = skills.description(row.text), text = row.text, files = files }
end

-- The skills, by name.
function skills.all(): { Skill }
  local list: { Skill } = {}
  for _, row in db:all(`select {COLUMNS} from skills order by name, id`) do
    table.insert(list, skillOf(row))
  end
  return list
end

function skills.find(id: string): Skill?
  local row = db:one(`select {COLUMNS} from skills where id = :id`, { id = id })
  return if row then skillOf(row) else nil
end

function skills.named(name: string): Skill?
  local row = db:one(`select {COLUMNS} from skills where name = :name`, { name = name })
  return if row then skillOf(row) else nil
end

-- A name a folder can have, on any machine: letters, digits, `.`, `_` and `-`, starting with a letter or a digit.
local NAME = "^[%w][%w%._%-]*$"

-- What is wrong with a path in the skill’s folder, in the person's words; nil when nothing is.
local function badPath(path: string): string?
  if path == "" then
    return "Give each file of the skill its path."
  end
  if string.sub(path, 1, 1) == "/" or string.find(path, "\\", 1, true) then
    return `{path} must be a path in the skill’s folder, with / between its parts, like scripts/check.sh.`
  end
  for part in string.gmatch(path .. "/", "([^/]*)/") do
    if part == "" or part == "." or part == ".." then
      return `{path} must be a path in the skill’s folder, with no empty part, no . and no ..`
    end
  end
  if path == "SKILL.md" then
    return "SKILL.md is the skill’s text: give the other files other paths."
  end
  return nil
end

-- The name, text and files as they are kept, or what is wrong with them in the person's words. `except` is the skill
-- being changed, whose own name is no other skill’s.
local function checked(name: string, text: string, files: { File }?, except: string?): (string, { File }?, string?)
  name = strings.trim(name)
  if name == "" then
    return name, nil, "Give the skill a name."
  end
  if #name > 64 or not string.match(name, NAME) then
    return name,
      nil,
      "A skill’s name is the name of its folder: letters, digits, ., _ and -, starting with a letter or a digit."
  end
  local taken = db:one("select 1 as found from skills where name = :n and id <> :id", { n = name, id = except or "" })
  if taken then
    return name, nil, `There is a skill named {name} already.`
  end
  if strings.trim(text) == "" then
    return name, nil, "Write the skill’s SKILL.md."
  end
  local kept: { File } = {}
  local seen: { [string]: boolean } = {}
  local given: { File } = files or {}
  for _, file in given do
    local path = strings.trim(file.path)
    local wrong = badPath(path)
    if wrong then
      return name, nil, wrong
    end
    if seen[path] then
      return name, nil, `The skill has two files at {path}.`
    end
    seen[path] = true
    table.insert(kept, { path = path, text = file.text })
  end
  return name, kept, nil
end

-- Add a skill. Returns the problem, when there is one, in the person's words.
function skills.add(name: string, text: string, files: { File }?): string?
  local kept, list, problem = checked(name, text, files, nil)
  if not list then
    return problem
  end
  db:exec(
    "insert into skills(id, name, text, files) values (:id, :name, :text, :files)",
    { id = random.uuid_v4(), name = kept, text = text, files = codec.encode(list) }
  )
  return nil
end

function skills.update(skill: Skill, name: string, text: string, files: { File }?): string?
  local kept, list, problem = checked(name, text, files, skill.id)
  if not list then
    return problem
  end
  db:exec(
    "update skills set name = :name, text = :text, files = :files where id = :id",
    { id = skill.id, name = kept, text = text, files = codec.encode(list) }
  )
  return nil
end

function skills.remove(skill: Skill)
  db:exec("delete from skills where id = :id", { id = skill.id })
end

return skills
