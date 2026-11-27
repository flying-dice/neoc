--!strict
-- The agents the person added, in the app's database: a name, the address of an ACP agent (a WebSocket URL, as
-- `acpd` serves one), the folder its sessions work in, on the agent's machine, and its picture (a small square image,
-- as a `data:` URL the page made, or empty for none). An agent's bearer token is not
-- there: it is in the operating system's keychain (`vnd:keyring`), read when the app connects to the agent and at no
-- other time, and the database only says whether there is one.
local keyring = require("vnd:keyring")
local random = require("lib:random")
local url = require("lib:url")
local db = app.db

-- the keychain items are `vos Agents` / the agent's id
local SERVICE = "vos Agents"

export type Agent = { id: string, name: string, url: string, token: boolean, folder: string, avatar: string }

local roster = {}

local function agentOf(row: { [string]: any }): Agent
  return {
    id = row.id,
    name = row.name,
    url = row.url,
    token = row.has_token == 1,
    folder = row.folder,
    avatar = row.avatar,
  }
end

function roster.all(): { agents_Agent }
  local list: { agents_Agent } = {}
  for _, row in db:all("select id, name, url, has_token, folder, avatar from agents order by name collate nocase, id") do
    table.insert(list, agentOf(row))
  end
  return list
end

function roster.find(id: string): Agent?
  local row = db:one("select id, name, url, has_token, folder, avatar from agents where id = :id", { id = id })
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

-- The agent's token. Neither a token nor a problem: it has none.
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

-- The folder as it is kept: blank is the root of the agent's machine.
local function folderOf(folder: string?): string
  local trimmed = string.match(folder or "", "^%s*(.-)%s*$") :: string
  return if trimmed == "" then "/" else trimmed
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

-- What is wrong with a name, an address, a folder and a new picture, in the person's words; nil when nothing is.
local function refusal(name: string, endpoint: string, folder: string, avatar: string?): string?
  if name == "" then
    return "Give the agent a name."
  end
  -- a tool is told the agent's name, and takes no more than a short one
  if #name > 100 then
    return "Give the agent a shorter name."
  end
  local parsed = url.parse(endpoint)
  if not parsed or (parsed.scheme ~= "ws" and parsed.scheme ~= "wss") or (parsed.host or "") == "" then
    return "The address must be a ws:// or wss:// URL, like ws://example.com:5424/acp."
  end
  -- a password in the URL would be kept in the database, and shown: the token field is the place for a secret
  if (parsed.username or "") ~= "" or (parsed.password or "") ~= "" then
    return "Leave the user name and password out of the URL: put the agent's token in the token field."
  end
  -- the session's folder is on the agent's machine, which may be Windows: `/home/me`, `C:\work` or `\\server\share`
  if not (string.match(folder, "^/") or string.match(folder, "^%a:[\\/]") or string.match(folder, "^\\\\")) then
    return "The folder must be an absolute path on the agent's machine, like /home/me/project."
  end
  if avatar and avatar ~= "" then
    if #avatar > AVATAR_MAX then
      return "The picture is too big. Choose a smaller one."
    end
    if not picture(avatar) then
      return "The picture must be a PNG, JPEG or WebP image."
    end
  end
  return nil
end

local function trimmed(text: string): string
  return string.match(text, "^%s*(.-)%s*$") :: string
end

-- Add an agent. Returns the problem, when there is one, in the person's words.
function roster.add(name: string, endpoint: string, token: string?, folder: string?, avatar: string?): string?
  name, endpoint = trimmed(name), trimmed(endpoint)
  local where = folderOf(folder)
  local problem = refusal(name, endpoint, where, avatar)
  if problem then
    return problem
  end
  local id = random.uuid_v4()
  if token and token ~= "" then
    local found, keychain = entry(id)
    if not found then
      return keychain
    end
    local ok, err = found:set_password(token)
    if not ok then
      return sentence(err)
    end
  end
  db:exec(
    [[insert into agents(id, name, url, has_token, folder, avatar)
      values (:id, :name, :url, :token, :folder, :avatar)]],
    {
      id = id,
      name = name,
      url = endpoint,
      token = if token and token ~= "" then 1 else 0,
      folder = where,
      avatar = avatar or "",
    }
  )
  return nil
end

-- Change an agent: a new token replaces the old one, `forgetToken` removes it, and with neither it stays. So with its
-- picture: a new one replaces it, `removeAvatar` removes it, and with neither it stays.
function roster.update(
  agent: Agent,
  name: string,
  endpoint: string,
  token: string?,
  forgetToken: boolean,
  folder: string?,
  avatar: string?,
  removeAvatar: boolean
): string?
  name, endpoint = trimmed(name), trimmed(endpoint)
  local where = folderOf(folder)
  local problem = refusal(name, endpoint, where, avatar)
  if problem then
    return problem
  end
  local has = agent.token
  if token and token ~= "" then
    local found, keychain = entry(agent.id)
    if not found then
      return keychain
    end
    local ok, err = found:set_password(token)
    if not ok then
      return sentence(err)
    end
    has = true
  elseif forgetToken then
    roster.forget(agent.id)
    has = false
  end
  local kept = if avatar and avatar ~= "" then avatar elseif removeAvatar then "" else agent.avatar
  db:tx(function()
    db:exec(
      [[update agents set name = :name, url = :url, has_token = :token, folder = :folder, avatar = :avatar
        where id = :id]],
      { id = agent.id, name = name, url = endpoint, token = if has then 1 else 0, folder = where, avatar = kept }
    )
    -- its chats are listed under its name
    db:exec("update threads set agent_name = :name where agent_id = :id", { id = agent.id, name = name })
    return nil
  end)
  return nil
end

function roster.remove(agent: Agent)
  roster.forget(agent.id)
  db:exec("delete from agents where id = :id", { id = agent.id })
end

return roster
