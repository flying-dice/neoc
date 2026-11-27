--!strict
-- App 1's backend, on the one RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau` (nothing in
-- it runs, and nothing is required). `add` and `clear` are public: App 1's page, other apps and agents (the tools
-- `app1__add` and `app1__clear`) reach them. `clear` first asks the person: `_client_confirm` is served by App 1's open
-- windows (the first answer wins). Each change is announced: `added` and `cleared` are public, so App 2 hears them;
-- `_saved` is private, for App 1's own page.

local items: { app1_Item } = {}
local nextId = 1

local function saved()
  RPC.send_notification("_saved", { total = #items })
end

-- who is calling: the person at a page, another app's backend, or an agent
local function by(ctx: vos_app_Context): string
  local actor = ctx.actor
  if actor.kind == "app" then
    return "app:" .. actor.id
  elseif actor.kind == "agent" then
    return "agent:" .. actor.name
  end
  return "you"
end

RPC.on_request("add", function(params: app1_AddParams, ctx: vos_app_Context): app1_AddResult
  local item: app1_Item = { id = nextId, text = params.text, by = by(ctx) }
  nextId += 1
  table.insert(items, item)
  RPC.send_notification("added", { item = item })
  saved()
  return item
end)

RPC.on_request("clear", function(_params: {}, ctx: vos_app_Context): app1_ClearResult
  local question = "Clear all " .. tostring(#items) .. " items? (asked by " .. by(ctx) .. ")"
  local ok: app1_ClientConfirmResult?, err = RPC.send_request("app1", "_client_confirm", { question = question })
  if err ~= nil then
    return ctx:fail("Nobody could confirm it: no App 1 window is open (" .. err.kind .. ")")
  end
  if not ok then
    return ctx:fail("Not cleared: the person said no")
  end
  local count = #items
  table.clear(items)
  RPC.send_notification("cleared", { count = count })
  saved()
  return count
end)

RPC.on_request("_items", function(): app1_ItemsResult
  return items
end)
