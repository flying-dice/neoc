--!strict
-- The agents the person added, in the app's database. Every agent has a name, its picture (a small square image, as a
-- `data:` URL the page made, or empty for none), the folder its sessions work in, and the MCP servers they are given.
-- How a chat reaches it is its type, fixed once it is added (transport.luau):
--
-- - `remote`: an ACP agent at an address (a WebSocket URL, as `acpd` serves one); the folder is on its machine. Its
--   bearer token is not in the database: it is in the operating system's keychain (`vnd:keyring`), read when the app
--   connects to the agent and at no other time, and the database only says whether there is one.
-- - `container`: each chat gets a container of its own (sandbox.luau), made from an image the Containers app built or
--   pulled, running the agent's command, with the skills of the Skills app it uses copied in; the folder is in the
--   container, where the chat's volume is mounted.
-- - `program`: each chat runs the agent's command on this machine, in the folder (which must be there).
--
-- The environment variables of a container's or a program's command, and the headers of an MCP server, are written
-- out or are the password of an item of the keychain the Keychain app keeps, named by its service and account. Such a
-- password is never in the database: it is read through `vnd:keyring` when a program or a container is started, or a
-- session is given its MCP servers (`resolve`).
local codec = require("./codec")
local fs = require("std:fs")
local keyring = require("vnd:keyring")
local random = require("lib:random")
local strings = require("lib:text")
local url = require("lib:url")
local db = app.db

-- the keychain items of remote agents' tokens are `vos Agents` / the agent's id
local SERVICE = "vos Agents"

export type Kind = agents_Kind
export type Variable = agents_Variable
export type Server = agents_Server
export type Agent = agents_Agent
-- What the page gives of an agent when it adds or changes one: what its type does not take is not kept.
export type Fields = {
  name: string,
  url: string?,
  folder: string?,
  command: { string }?,
  image: string?,
  skills_dir: string?,
  skills: { string }?,
  env: { Variable }?,
  mcp: { Server }?,
}
-- A name and its value, read: what a program's or a container's environment and an MCP server's headers are given.
export type Pair = { name: string, value: string }

-- the folders a container agent has when none is given
local WORKDIR = "/workspace"
local SKILLS_DIR = "/root/.claude/skills"
-- an environment variable's name, as a shell takes it
local ENV_NAME = "^[A-Za-z_][A-Za-z0-9_]*$"
-- a header's name: no spaces and no `:`
local HEADER_NAME = "^[^%s:]+$"
-- a skill's name, which is the name of its folder: letters, digits, `.`, `_` and `-`, starting with a letter or a digit
local SKILL_NAME = "^[%w][%w%._%-]*$"
-- an image by name and tag: no spaces, starting with a letter or a digit
local IMAGE_NAME = "^[%w][%w%._%-/:@]*$"

local COLUMNS = "id, name, kind, url, has_token, folder, avatar, command, image, skills_dir, skills, env, mcp"

local roster = {}

local function list(text: string): { any }
  local value = codec.decode(text)
  return if type(value) == "table" then value else {}
end

local function agentOf(row: { [string]: any }): Agent
  return {
    id = row.id,
    name = row.name,
    kind = row.kind,
    url = row.url,
    token = row.has_token == 1,
    folder = row.folder,
    avatar = row.avatar,
    command = list(row.command),
    image = row.image,
    skills_dir = row.skills_dir,
    skills = list(row.skills),
    env = list(row.env),
    mcp = list(row.mcp),
  }
end

function roster.all(): { Agent }
  local out: { Agent } = {}
  for _, row in db:all(`select {COLUMNS} from agents order by name collate nocase, id`) do
    table.insert(out, agentOf(row))
  end
  return out
end

function roster.find(id: string): Agent?
  local row = db:one(`select {COLUMNS} from agents where id = :id`, { id = id })
  return if row then agentOf(row) else nil
end

-- the keychain's own message
local function sentence(err: vnd_keyring_Error?): string
  return `The keychain did not take the token: {if err then err.message else "it did not say why"}`
end

local function entry(id: string): (vnd_keyring_Entry?, string?)
  local store, storeErr = keyring.store()
  if not store then
    return nil, sentence(storeErr)
  end
  local found, err = store:entry(SERVICE, id)
  if not found then
    return nil, sentence(err)
  end
  return found, nil
end

-- A remote agent's token. Neither a token nor a problem: it has none.
function roster.token(agent: Agent): (string?, string?)
  if not agent.token then
    return nil, nil
  end
  local found, problem = entry(agent.id)
  if not found then
    return nil, problem
  end
  local token, err = found:get_password()
  if err then
    return nil, `The keychain did not give the agent's token: {err}`
  end
  if not token then
    return nil, "The agent's token is no longer in the keychain. Give it again in the agent's settings."
  end
  return token, nil
end

-- Remove the token kept for the agent of that id, when there is one.
function roster.forget(id: string)
  local found = entry(id)
  if found then
    found:delete_credential()
  end
end

-- The longest picture kept: the page sends a 256-pixel square, a few tens of thousands of characters.
local AVATAR_MAX = 200000

-- the kinds of picture kept: what a browser shows in an `img`, and never SVG (a picture that can hold a script)
local AVATAR_PREFIXES = { "data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64," }

-- Whether a picture is a PNG, JPEG or WebP `data:` URL in base64.
local function picture(avatar: string): boolean
  for _, prefix in AVATAR_PREFIXES do
    if string.sub(avatar, 1, #prefix) == prefix then
      return string.match(string.sub(avatar, #prefix + 1), "^[A-Za-z0-9+/]+=?=?$") ~= nil
    end
  end
  return false
end

-- Whether a path is absolute on a machine that may be Windows: `/home/me`, `C:\work` or `\\server\share`.
local function absolute(path: string): boolean
  return string.match(path, "^/") ~= nil or string.match(path, "^%a:[\\/]") ~= nil or string.match(path, "^\\\\") ~= nil
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
    -- the app's own server, with this machine's tools, is `vos`
    if name == "vos" then
      return nil, "vos is the name of this machine's own MCP server: give yours another name."
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

-- The agent's command, or nil when its first word is blank.
local function commandOf(given: { string }?): { string }?
  local command: { string } = given or {}
  if #command == 0 or strings.trim(command[1]) == "" then
    return nil
  end
  local out = table.clone(command)
  out[1] = strings.trim(out[1])
  return out
end

-- What is wrong with a remote agent's address, in the person's words; nil when nothing is.
local function badAddress(endpoint: string): string?
  local parsed = url.parse(endpoint)
  if not parsed or (parsed.scheme ~= "ws" and parsed.scheme ~= "wss") or (parsed.host or "") == "" then
    return "The address must be a ws:// or wss:// URL, like ws://example.com:5424/acp."
  end
  -- a password in the URL would be kept in the database, and shown: the token field is the place for a secret
  if (parsed.username or "") ~= "" or (parsed.password or "") ~= "" then
    return "Leave the user name and password out of the URL: put the agent's token in the token field."
  end
  return nil
end

-- What is wrong with a program agent's folder, in the person's words: it must be a folder of this machine.
local function badHostFolder(folder: string): string?
  if folder == "" then
    return "Give the folder the program works in, on this machine, like /home/me/project."
  end
  if not absolute(folder) then
    return "The folder must be an absolute path on this machine, like /home/me/project."
  end
  local meta = fs.metadata(folder)
  if not meta or meta.kind ~= "dir" then
    return `There is no folder {folder} on this machine.`
  end
  return nil
end

-- The columns of an agent of `kind` as they are kept, or what is wrong with the fields in the person's words.
local function checked(kind: Kind, fields: Fields, avatar: string?): ({ [string]: any }?, string?)
  local name = strings.trim(fields.name)
  if name == "" then
    return nil, "Give the agent a name."
  end
  -- a tool is told the agent's name, and takes no more than a short one
  if #name > 100 then
    return nil, "Give the agent a shorter name."
  end
  local folder = strings.trim(fields.folder or "")
  local columns: { [string]: any } = {
    name = name,
    kind = kind,
    url = "",
    folder = folder,
    command = "[]",
    image = "",
    skills_dir = "",
    skills = "[]",
    env = "[]",
    mcp = "[]",
  }

  if kind == "remote" then
    local endpoint = strings.trim(fields.url or "")
    local wrong = badAddress(endpoint)
    if wrong then
      return nil, wrong
    end
    columns.url = endpoint
    columns.folder = if folder == "" then "/" else folder
    -- the session's folder is on the agent's machine, which may be Windows
    if not absolute(columns.folder) then
      return nil, "The folder must be an absolute path on the agent's machine, like /home/me/project."
    end
  else
    local command = commandOf(fields.command)
    if not command then
      return nil,
        if kind == "container"
          then "Give the agent's command: the program the container runs, like claude-code-acp."
          else "Give the agent's command: the program this machine runs, like claude-code-acp."
    end
    columns.command = codec.encode(command)
    local env, wrongEnv = variables(fields.env or {}, "the variables", ENV_NAME)
    if not env then
      return nil, wrongEnv
    end
    columns.env = codec.encode(env)
  end

  if kind == "program" then
    local wrong = badHostFolder(folder)
    if wrong then
      return nil, wrong
    end
  elseif kind == "container" then
    local image = strings.trim(fields.image or "")
    if image == "" then
      return nil, "Choose the image each chat's container is made from: build or pull it in Containers."
    end
    if not string.match(image, IMAGE_NAME) then
      return nil, `{image} cannot be an image's name: a name and a tag, like debian:stable.`
    end
    columns.image = image
    columns.folder = if folder == "" then WORKDIR else folder
    if string.sub(columns.folder, 1, 1) ~= "/" then
      return nil, "The work folder must be an absolute path in the container, like /workspace."
    end
    local skillsDir = strings.trim(fields.skills_dir or "")
    columns.skills_dir = if skillsDir == "" then SKILLS_DIR else skillsDir
    if string.sub(columns.skills_dir, 1, 1) ~= "/" then
      return nil, "The skills folder must be an absolute path in the container, like /root/.claude/skills."
    end
    local skills: { string } = {}
    local given: { string } = fields.skills or {}
    for _, skill in given do
      local named = strings.trim(skill)
      if #named > 64 or not string.match(named, SKILL_NAME) then
        return nil, `{named} cannot be a skill's name: letters, digits, ., _ and -, starting with a letter or a digit.`
      end
      if not table.find(skills, named) then
        table.insert(skills, named)
      end
    end
    columns.skills = codec.encode(skills)
  end

  local mcp, wrongMcp = servers(fields.mcp or {})
  if not mcp then
    return nil, wrongMcp
  end
  columns.mcp = codec.encode(mcp)
  if avatar and avatar ~= "" then
    if #avatar > AVATAR_MAX then
      return nil, "The picture is too big. Choose a smaller one."
    end
    if not picture(avatar) then
      return nil, "The picture must be a PNG, JPEG or WebP image."
    end
  end
  return columns, nil
end

-- Add an agent of `kind`. Returns the problem, when there is one, in the person's words.
function roster.add(kind: Kind, fields: Fields, token: string?, avatar: string?): string?
  local columns, problem = checked(kind, fields, avatar)
  if not columns then
    return problem
  end
  local id = random.uuid_v4()
  -- a token is a remote agent's
  local secret = if kind == "remote" and token and token ~= "" then token else nil
  if secret then
    local found, keychain = entry(id)
    if not found then
      return keychain
    end
    local ok, err = found:set_password(secret)
    if not ok then
      return sentence(err)
    end
  end
  columns.id = id
  columns.token = if secret then 1 else 0
  columns.avatar = avatar or ""
  db:exec(
    [[insert into agents(id, name, kind, url, has_token, folder, avatar, command, image, skills_dir, skills, env, mcp)
      values (:id, :name, :kind, :url, :token, :folder, :avatar, :command, :image, :skills_dir, :skills, :env, :mcp)]],
    columns
  )
  return nil
end

-- Change an agent; its type stays. A new token replaces the old one, `forgetToken` removes it, and with neither it
-- stays. So with its picture: a new one replaces it, `removeAvatar` removes it, and with neither it stays.
function roster.update(
  agent: Agent,
  fields: Fields,
  token: string?,
  forgetToken: boolean,
  avatar: string?,
  removeAvatar: boolean
): string?
  local columns, problem = checked(agent.kind, fields, avatar)
  if not columns then
    return problem
  end
  local has = agent.token
  if agent.kind == "remote" and token and token ~= "" then
    local found, keychain = entry(agent.id)
    if not found then
      return keychain
    end
    local ok, err = found:set_password(token)
    if not ok then
      return sentence(err)
    end
    has = true
  elseif forgetToken and has then
    roster.forget(agent.id)
    has = false
  end
  columns.id = agent.id
  columns.token = if has then 1 else 0
  columns.avatar = if avatar and avatar ~= "" then avatar elseif removeAvatar then "" else agent.avatar
  db:tx(function()
    db:exec(
      [[update agents set name = :name, url = :url, has_token = :token, folder = :folder, avatar = :avatar,
          command = :command, image = :image, skills_dir = :skills_dir, skills = :skills, env = :env, mcp = :mcp
        where id = :id]],
      columns
    )
    -- its chats are listed under its name
    db:exec("update threads set agent_name = :name where agent_id = :id", { id = agent.id, name = columns.name })
    return nil
  end)
  return nil
end

function roster.remove(agent: Agent)
  if agent.token then
    roster.forget(agent.id)
  end
  db:exec("delete from agents where id = :id", { id = agent.id })
end

-- The values of variables: those written out as they are, those of the keychain read from it now. Otherwise why one
-- has none, in the person's words.
function roster.resolve(rows: { Variable }): ({ Pair }?, string?)
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
      local found, err = (store :: vnd_keyring_Store):entry(secret.service, secret.account)
      if not found then
        return nil, `The keychain refused the item of {row.name}: {if err then err.message else "it did not say why"}`
      end
      local password, unread = found:get_password()
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

return roster
