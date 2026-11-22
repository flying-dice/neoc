--!strict
-- The Keychain app's backend: the passwords the machine's own keychain keeps, as a list of the items the person put
-- on it (items.luau). The passwords stay in the keychain (vault.luau): one is read when the person asks to see it,
-- and at no other time. Every call that changes something answers with the overview as it now stands, so the window
-- that asked draws what the backend has.
--
-- An agent can see which items there are (`list_keychain_items`: the service and the account of each, never a
-- password) and keep a new password (`add_keychain_item`). Reading a password and deleting an item are left to the
-- person. A tool that changed something tells the open windows with a `changed` event, and they read the overview
-- again.
local app = require("vos:app")
local items = require("./items")
local rpc = require("../.app/rpc")
local t = app.t

local handlers: rpc.Handlers = {
  overview = function(_ctx)
    return items.overview()
  end,

  add = function(ctx, params)
    local problem = items.add(params.service, params.account, params.password)
    return if problem then ctx:fail(problem) else items.overview()
  end,

  reveal = function(ctx, params)
    local password, problem = items.reveal(params.item)
    return if password then { password = password } else ctx:fail(problem or "The keychain no longer has this item.")
  end,

  remove = function(ctx, params)
    local problem = items.remove(params.item)
    return if problem then ctx:fail(problem) else items.overview()
  end,
}

app.tool {
  name = "list_keychain_items",
  title = "List the keychain items",
  readOnly = true,
  description = "The items of this machine's keychain that the Keychain app shows: what each password is for (`service`) and whose it is there (`account`). No password is given: only the person can look at one, in the app.",
  input = {},
  run = function(_input: {}, _ctx)
    local listed: { { service: string, account: string } } = {}
    for _, item in items.all() do
      table.insert(listed, { service = item.service, account = item.account })
    end
    return { items = listed }
  end,
}

app.tool {
  name = "add_keychain_item",
  title = "Keep a password in the keychain",
  description = "Keep a new password in this machine's keychain, under what it is for and whose it is, and show the item in the Keychain app. A password the keychain has already for that service and account is not replaced: the call fails and says so.",
  input = {
    service = t.string { description = "What the password is for: a site, a program, a machine", maxLength = 200 },
    account = t.string { description = "Whose password it is there: a user name, an email address", maxLength = 200 },
    password = t.string { description = "The password to keep", minLength = 1, maxLength = 4000 },
  },
  run = function(input: { service: string, account: string, password: string }, ctx)
    local problem = items.add(input.service, input.account, input.password)
    if problem then
      return ctx:fail(problem)
    end
    app.emit("changed")
    return { service = input.service, account = input.account }
  end,
}

return handlers
