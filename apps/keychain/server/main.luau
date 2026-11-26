--!strict
-- The Keychain app's backend: the passwords the machine's own keychain keeps, as a list of the items the person put
-- on it (items.luau). The passwords stay in the keychain (vault.luau): one is read when the person asks to see it,
-- and at no other time. Every call that changes something answers with the overview as it now stands, so the window
-- that asked draws what the backend has. It serves on the RPC bus through the global `RPC`, typed by the generated
-- `types/vos.d.luau`.
--
-- The page's methods are private (`_overview`, `_add`, `_reveal`, `_remove`). An agent can see which items there are
-- (`list_keychain_items`: the service and the account of each, never a password) and keep a new password
-- (`add_keychain_item`). Reading a password and deleting an item are left to the person. A tool that changed something
-- tells the open windows with the `changed` notification, and they read the overview again.
local items = require("./items")

RPC.on_request("_overview", function(): keychain_OverviewResult
  return items.overview()
end)

RPC.on_request("_add", function(params: keychain_AddParams, ctx: vos_app_Context): keychain_AddResult
  local problem = items.add(params.service, params.account, params.password)
  return if problem then ctx:fail(problem) else items.overview()
end)

RPC.on_request("_reveal", function(params: keychain_RevealParams, ctx: vos_app_Context): keychain_RevealResult
  local password, problem = items.reveal(params.item)
  return if password then { password = password } else ctx:fail(problem or "The keychain no longer has this item.")
end)

RPC.on_request("_remove", function(params: keychain_RemoveParams, ctx: vos_app_Context): keychain_RemoveResult
  local problem = items.remove(params.item)
  return if problem then ctx:fail(problem) else items.overview()
end)

RPC.on_request("list_keychain_items", function(): keychain_ListKeychainItemsResult
  local listed: { keychain_Entry } = {}
  for _, item in items.all() do
    table.insert(listed, { service = item.service, account = item.account })
  end
  return { items = listed }
end)

RPC.on_request(
  "add_keychain_item",
  function(params: keychain_AddKeychainItemParams, ctx: vos_app_Context): keychain_AddKeychainItemResult
    local problem = items.add(params.service, params.account, params.password)
    if problem then
      return ctx:fail(problem)
    end
    RPC.send_notification("changed")
    return { service = params.service, account = params.account }
  end
)
