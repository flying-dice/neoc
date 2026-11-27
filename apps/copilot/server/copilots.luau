--!strict
-- The copilots the person defined, in the app's database. A copilot is what a chat's container is made from and what
-- runs in it: a Dockerfile (its image is images.luau's), the agent's command, the folder the agent works in (each
-- chat's volume is mounted there) and the folder its skills go in, the container's environment variables, the MCP
-- servers its sessions are given, and the skills of the library (skills.luau) it uses.
--
-- A variable's value, and the value of an MCP server's header, is written out or is the password of an item of the
-- keychain the Keychain app keeps, named by its service and account. Such a password is never in the database: it is
-- read through `vnd:keyring` when a chat's container is made or a session is given its MCP servers (`resolve`).
local codec = require("./codec")
local images = require("./images")
local keyring = require("vnd:keyring")
local random = require("lib:random")
local strings = require("lib:text")
local url = require("lib:url")
local db = app.db

export type Secret = copilot_Secret
export type Variable = copilot_Variable
export type Server = copilot_Server
-- A copilot as the database has it.
export type Copilot = {
  id: string,
  name: string,
  dockerfile: string,
  command: { string },
  workdir: string,
  skills_dir: string,
  env: { Variable },
  mcp: { Server },
  skills: { string },
  image: string,
  problem: string,
}
-- What the page gives of a copilot when it adds or changes one.
export type Fields = {
  name: string,
  dockerfile: string,
  command: { string },
  workdir: string?,
  skills_dir: string?,
  env: { Variable }?,
  mcp: { Server }?,
  skills: { string }?,
}
-- A name and its value, read: what a container's environment and an MCP server's headers are given.
export type Pair = { name: string, value: string }

local WORKDIR = "/workspace"
local SKILLS_DIR = "/root/.claude/skills"
-- an environment variable's name, as a shell takes it
local ENV_NAME = "^[A-Za-z_][A-Za-z0-9_]*$"
-- a header's name: no spaces and no `:`
local HEADER_NAME = "^[^%s:]+$"

local copilots = {}

local COLUMNS = "id, name, dockerfile, command, workdir, skills_dir, env, mcp, image, problem"

local function list(text: string): { any }
  local value = codec.decode(text)
  return if type(value) == "table" then value else {}
end

-- the ids of the skills each copilot uses, in the order of the skills' names
local function chosen(): { [string]: { string } }
  local by: { [string]: { string } } = {}
  for _, row in
    db:all(
      "select copilot_skills.copilot_id, copilot_skills.skill_id from copilot_skills"
        .. " join skills on skills.id = copilot_skills.skill_id order by skills.name, skills.id"
    )
  do
    local ids = by[row.copilot_id] or {}
    by[row.copilot_id] = ids
    table.insert(ids, row.skill_id)
  end
  return by
end

local function copilotOf(row: { [string]: any }, skills: { string }): Copilot
  return {
    id = row.id,
    name = row.name,
    dockerfile = row.dockerfile,
    command = list(row.command),
    workdir = row.workdir,
    skills_dir = row.skills_dir,
    env = list(row.env),
    mcp = list(row.mcp),
    skills = skills,
    image = row.image,
    problem = row.problem,
  }
end

function copilots.all(): { copilot_Copilot }
  local skills = chosen()
  local out: { copilot_Copilot } = {}
  for _, row in db:all(`select {COLUMNS} from copilots order by name collate nocase, id`) do
    local copilot = copilotOf(row, skills[row.id] or {})
    table.insert(out, {
      id = copilot.id,
      name = copilot.name,
      dockerfile = copilot.dockerfile,
      command = copilot.command,
      workdir = copilot.workdir,
      skills_dir = copilot.skills_dir,
      env = copilot.env,
      mcp = copilot.mcp,
      skills = copilot.skills,
      image = copilot.image,
      current = copilot.image ~= "" and copilot.image == images.tag(copilot.id, copilot.dockerfile),
      building = images.building(copilot.id),
      problem = copilot.problem,
    })
  end
  return out
end

function copilots.find(id: string): Copilot?
  local row = db:one(`select {COLUMNS} from copilots where id = :id`, { id = id })
  if not row then
    return nil
  end
  return copilotOf(row, chosen()[id] or {})
end

-- A folder in the container as it is kept: blank is `fallback`. Nil when it is not an absolute path.
local function folderOf(folder: string?, fallback: string): string?
  local trimmed = strings.trim(folder or "")
  if trimmed == "" then
    return fallback
  end
  return if string.sub(trimmed, 1, 1) == "/" then trimmed else nil
end

-- Variables as they are kept, or what is wrong with them. `what` says whose they are, `pattern` what a name is.
local function variables(rows: { Variable }, what: string, pattern: string): ({ Variable }?, string?)
  local kept: { Variable } = {}
  local seen: { [string]: boolean } = {}
  for _, row in rows do
    local name = strings.trim(row.name)
    if name == "" then
      return nil, `Give each of {what} a name.`
    end
    if not string.match(name, pattern) then
      return nil,
        if pattern == ENV_NAME
          then `{name} cannot be the name of an environment variable: letters, digits and _, not starting with a digit.`
          else `{name} cannot be the name of a header.`
    end
    if seen[name] then
      return nil, `Two of {what} are named {name}.`
    end
    seen[name] = true
    local secret = row.secret
    if secret then
      if row.value ~= nil then
        return nil, `{name} has a value and a keychain item: give it one of them.`
      end
      local service = strings.trim(secret.service)
      local account = strings.trim(secret.account)
      if service == "" or account == "" then
        return nil, `{name} takes its value from the keychain: choose the item, by its service and account.`
      end
      table.insert(kept, { name = name, secret = { service = service, account = account } })
    else
      table.insert(kept, { name = name, value = row.value or "" })
    end
  end
  return kept, nil
end

-- MCP servers as they are kept, or what is wrong with them.
local function servers(given: { Server }): ({ Server }?, string?)
  local kept: { Server } = {}
  local seen: { [string]: boolean } = {}
  for _, server in given do
    local name = strings.trim(server.name)
    if name == "" then
      return nil, "Give each MCP server a name."
    end
    if seen[name] then
      return nil, `Two MCP servers are named {name}.`
    end
    seen[name] = true
    if server.type == "stdio" then
      local command = strings.trim(server.command or "")
      if command == "" then
        return nil, `Give the MCP server {name} the program it runs.`
      end
      local env, problem = variables(server.env or {}, `the variables of {name}`, ENV_NAME)
      if not env then
        return nil, problem
      end
      table.insert(kept, { type = "stdio", name = name, command = command, args = server.args or {}, env = env })
    else
      local address = strings.trim(server.url or "")
      local parsed = url.parse(address)
      if not parsed or (parsed.scheme ~= "http" and parsed.scheme ~= "https") or (parsed.host or "") == "" then
        return nil, `The address of the MCP server {name} must be an http:// or https:// URL.`
      end
      local headers, problem = variables(server.headers or {}, `the headers of {name}`, HEADER_NAME)
      if not headers then
        return nil, problem
      end
      table.insert(kept, { type = "http", name = name, url = address, headers = headers })
    end
  end
  return kept, nil
end

-- What is kept of a copilot: its columns, and the skills it uses. What is wrong with the fields, in the person's
-- words, when something is.
type Kept = { columns: { [string]: any }, skills: { string } }
local function checked(fields: Fields): (Kept?, string?)
  local name = strings.trim(fields.name)
  if name == "" then
    return nil, "Give the copilot a name."
  end
  if strings.trim(fields.dockerfile) == "" then
    return nil, "Write the copilot's Dockerfile."
  end
  local command = fields.command
  if #command == 0 or strings.trim(command[1]) == "" then
    return nil, "Give the agent's command: the program the container runs, like claude-code-acp."
  end
  local workdir = folderOf(fields.workdir, WORKDIR)
  if not workdir then
    return nil, "The work folder must be an absolute path in the container, like /workspace."
  end
  local skillsDir = folderOf(fields.skills_dir, SKILLS_DIR)
  if not skillsDir then
    return nil, "The skills folder must be an absolute path in the container, like /root/.claude/skills."
  end
  local env, wrongEnv = variables(fields.env or {}, "the variables", ENV_NAME)
  if not env then
    return nil, wrongEnv
  end
  local mcp, wrongMcp = servers(fields.mcp or {})
  if not mcp then
    return nil, wrongMcp
  end
  local skills: { string } = {}
  local given: { string } = fields.skills or {}
  for _, id in given do
    if not db:one("select 1 as found from skills where id = :id", { id = id }) then
      return nil, "One of the skills is no longer in the library."
    end
    if not table.find(skills, id) then
      table.insert(skills, id)
    end
  end
  return {
    columns = {
      name = name,
      dockerfile = fields.dockerfile,
      command = codec.encode(command),
      workdir = workdir,
      skills_dir = skillsDir,
      env = codec.encode(env),
      mcp = codec.encode(mcp),
    },
    skills = skills,
  },
    nil
end

local function choose(id: string, skills: { string })
  db:exec("delete from copilot_skills where copilot_id = :id", { id = id })
  for _, skill in skills do
    db:exec("insert into copilot_skills(copilot_id, skill_id) values (:c, :s)", { c = id, s = skill })
  end
end

-- Add a copilot. Returns the problem, when there is one, in the person's words.
function copilots.add(fields: Fields): string?
  local kept, problem = checked(fields)
  if not kept then
    return problem
  end
  local id = random.uuid_v4()
  local columns = kept.columns
  columns.id = id
  db:tx(function()
    db:exec(
      [[insert into copilots(id, name, dockerfile, command, workdir, skills_dir, env, mcp)
        values (:id, :name, :dockerfile, :command, :workdir, :skills_dir, :env, :mcp)]],
      columns
    )
    choose(id, kept.skills)
    return nil
  end)
  return nil
end

-- Change a copilot. Its chats are listed under its name.
function copilots.update(copilot: Copilot, fields: Fields): string?
  local kept, problem = checked(fields)
  if not kept then
    return problem
  end
  local columns = kept.columns
  columns.id = copilot.id
  db:tx(function()
    db:exec(
      [[update copilots set name = :name, dockerfile = :dockerfile, command = :command, workdir = :workdir,
          skills_dir = :skills_dir, env = :env, mcp = :mcp
        where id = :id]],
      columns
    )
    choose(copilot.id, kept.skills)
    db:exec("update threads set copilot_name = :name where copilot_id = :id", { id = copilot.id, name = columns.name })
    return nil
  end)
  return nil
end

function copilots.remove(copilot: Copilot)
  db:tx(function()
    db:exec("delete from copilot_skills where copilot_id = :id", { id = copilot.id })
    db:exec("delete from copilots where id = :id", { id = copilot.id })
    return nil
  end)
end

-- The values of variables: those written out as they are, those of the keychain read from it now. Otherwise why one
-- has none, in the person's words.
function copilots.resolve(rows: { Variable }): ({ Pair }?, string?)
  local store: vnd_keyring_Store? = nil
  local out: { Pair } = {}
  for _, row in rows do
    local secret = row.secret
    if secret then
      if not store then
        local opened, err = keyring.store()
        if not opened then
          return nil,
            `The keychain could not be opened, so {row.name} has no value: {if err
              then err.message
              else "it did not say why"}`
        end
        store = opened
      end
      local entry, err = (store :: vnd_keyring_Store):entry(secret.service, secret.account)
      if not entry then
        return nil, `The keychain refused the item of {row.name}: {if err then err.message else "it did not say why"}`
      end
      local password, unread = entry:get_password()
      if unread then
        return nil, `The keychain did not give the value of {row.name}: {unread.message}`
      end
      if not password then
        return nil,
          `The keychain has no item for {secret.service} / {secret.account}, which {row.name} takes its value from.`
      end
      table.insert(out, { name = row.name, value = password })
    else
      table.insert(out, { name = row.name, value = row.value or "" })
    end
  end
  return out, nil
end

return copilots
