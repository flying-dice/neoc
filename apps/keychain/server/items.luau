--!strict
-- The items the app shows: which passwords of the keychain are on its list. The passwords themselves are the
-- keychain's (vault.luau) and never the app's: its database keeps the service and the account of each item, because
-- not every keychain can be asked what it holds, and nothing else.
--
-- A function that changes something answers with a plain sentence when it refused (the handler and the tool pass it
-- on as it is), and with nothing when it did what it was asked.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local strings = require("lib:text")
local vault = require("./vault")
local db = app.db

local items = {}

db:migrate {
  [1] = [[
    create table items (
      id integer primary key,
      service text not null,
      account text not null,
      unique (service, account)
    );
  ]],
}

local function itemOf(row: { [string]: any }): rpc.Item
  return { id = row.id, service = row.service, account = row.account }
end

function items.item(id: number): rpc.Item?
  local row = db:one("select id, service, account from items where id = :id", { id = id })
  return if row then itemOf(row) else nil
end

-- by service, then account, whatever their capitals
function items.all(): { rpc.Item }
  local all: { rpc.Item } = {}
  for _, row in
    db:all("select id, service, account from items order by service collate nocase, account collate nocase, id")
  do
    table.insert(all, itemOf(row))
  end
  return all
end

function items.overview(): rpc.Overview
  return { keychain = vault.status(), items = items.all() }
end

-- Put an item on the list. With a password it is a new item of the keychain: one the keychain has already is not
-- replaced, so that nothing another program keeps there is lost to a slip here. Without one it is an item the keychain
-- has already, shown as it is.
function items.add(service: string, account: string, password: string?): string?
  local what = strings.trim(service)
  local whose = strings.trim(account)
  if what == "" then
    return "Say what the password is for."
  elseif whose == "" then
    return "Say whose password it is."
  end
  if db:one("select id from items where service = :s and account = :a", { s = what, a = whose }) then
    return "This item is here already."
  end
  local kept, problem = vault.read(what, whose)
  if problem then
    return problem
  end
  if password == nil or password == "" then
    if kept == nil then
      return "The keychain has no password for this service and account. Type one to keep it."
    end
  else
    if kept ~= nil then
      return "The keychain has a password for this service and account already. Leave the password empty to show it here."
    end
    local refused = vault.write(what, whose, password)
    if refused then
      return refused
    end
  end
  db:exec("insert into items(service, account) values (:s, :a)", { s = what, a = whose })
  return nil
end

-- the password of an item, read from the keychain now
function items.reveal(id: number): (string?, string?)
  local item = items.item(id)
  if not item then
    return nil, "There is no such item."
  end
  local password, problem = vault.read(item.service, item.account)
  if problem then
    return nil, problem
  end
  return password, if password then nil else "The keychain no longer has this item."
end

-- delete an item from the keychain, and from the list
function items.remove(id: number): string?
  local item = items.item(id)
  if not item then
    return "There is no such item."
  end
  local problem = vault.delete(item.service, item.account)
  if problem then
    return problem
  end
  db:exec("delete from items where id = :id", { id = id })
  return nil
end

return items
