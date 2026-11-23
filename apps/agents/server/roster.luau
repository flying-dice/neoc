--!strict
-- The agents the person added: a name and an AG-UI endpoint each, in the app's database. An agent's bearer token is
-- not there: it is in the operating system's keychain (`vnd:keyring`), read when a run starts and at no other time,
-- and the database only says whether there is one.
local app = require("vos:app")
local keyring = require("vnd:keyring")
local random = require("lib:random")
local rpc = require("../.app/rpc")
local url = require("lib:url")
local db = app.db

-- the keychain items are `vos Agents` / the agent's id
local SERVICE = "vos Agents"

export type Agent = { id: string, name: string, url: string, token: boolean }

local roster = {}

local function agentOf(row: { [string]: any }): Agent
  return { id = row.id, name = row.name, url = row.url, token = row.has_token == 1 }
end

function roster.all(): { rpc.Agent }
  local list: { rpc.Agent } = {}
  for _, row in db:all("select id, name, url, has_token from agents order by name collate nocase, id") do
    table.insert(list, agentOf(row))
  end
  return list
end

function roster.find(id: string): Agent?
  local row = db:one("select id, name, url, has_token from agents where id = :id", { id = id })
  return if row then agentOf(row) else nil
end

-- the keychain's own message, without the module's prefix
local function sentence(err: string?): string
  local message = string.match(err or "", "^keyring%.[%a_]+: [%a_]+: (.*)$")
  return `The keychain did not take the token: {message or err or "it did not say why"}`
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

local function forget(id: string)
  local found = entry(id)
  if found then
    found:delete_credential()
  end
end

-- What is wrong with a name and an endpoint, in the person's words; nil when nothing is.
local function refusal(name: string, endpoint: string): string?
  if name == "" then
    return "Give the agent a name."
  end
  local parsed = url.parse(endpoint)
  if not parsed or (parsed.scheme ~= "http" and parsed.scheme ~= "https") or (parsed.host or "") == "" then
    return "The endpoint must be an http:// or https:// URL."
  end
  -- a password in the URL would be kept in the database, and shown: the token field is the place for a secret
  if (parsed.username or "") ~= "" or (parsed.password or "") ~= "" then
    return "Leave the user name and password out of the URL: put the agent's token in the token field."
  end
  return nil
end

local function trimmed(text: string): string
  return string.match(text, "^%s*(.-)%s*$") :: string
end

-- Add an agent. Returns the problem, when there is one, in the person's words.
function roster.add(name: string, endpoint: string, token: string?): string?
  name, endpoint = trimmed(name), trimmed(endpoint)
  local problem = refusal(name, endpoint)
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
    "insert into agents(id, name, url, has_token) values (:id, :name, :url, :token)",
    { id = id, name = name, url = endpoint, token = if token and token ~= "" then 1 else 0 }
  )
  return nil
end

-- Change an agent: a new token replaces the old one, `forgetToken` removes it, and with neither it stays.
function roster.update(agent: Agent, name: string, endpoint: string, token: string?, forgetToken: boolean): string?
  name, endpoint = trimmed(name), trimmed(endpoint)
  local problem = refusal(name, endpoint)
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
    forget(agent.id)
    has = false
  end
  db:tx(function()
    db:exec(
      "update agents set name = :name, url = :url, has_token = :token where id = :id",
      { id = agent.id, name = name, url = endpoint, token = if has then 1 else 0 }
    )
    -- its chats are listed under its name
    db:exec("update threads set agent_name = :name where agent_id = :id", { id = agent.id, name = name })
    return nil
  end)
  return nil
end

function roster.remove(agent: Agent)
  forget(agent.id)
  db:exec("delete from agents where id = :id", { id = agent.id })
end

return roster
