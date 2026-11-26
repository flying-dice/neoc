--!strict
-- App 1's backend, on the one RPC bus through the global `app` (nothing generated runs: `types/app1` is types). `add` and
-- `clear` are public: App 1's page, other apps and agents (the tools `app1__add` and `app1__clear`) reach them. `clear`
-- first asks the person: `_client_confirm` is served by App 1's open windows (the first answer wins). Each change is
-- announced: `added` and `cleared` are public, so App 2 hears them; `_saved` is private, for App 1's own page.
local app1 = require("../types/app1")

local items: { app1.Item } = {}
local nextId = 1

local function saved()
  app.sendNotification("_saved", { total = #items })
end

-- who is calling: the person at a page, another app's backend, or an agent
local function by(ctx: app1.Context): string
  local actor = ctx.actor
  if actor.kind == "app" then
    return "app:" .. actor.id
  elseif actor.kind == "agent" then
    return "agent:" .. actor.name
  end
  return "you"
end

app.onRequest("add", function(params: app1.AddParams, ctx: app1.Context): app1.AddResult
  local item: app1.Item = { id = nextId, text = params.text, by = by(ctx) }
  nextId += 1
  table.insert(items, item)
  app.sendNotification("added", { item = item })
  saved()
  return item
end)

app.onRequest("clear", function(_params: {}, ctx: app1.Context): app1.ClearResult
  local question = "Clear all " .. tostring(#items) .. " items? (asked by " .. by(ctx) .. ")"
  local ok: app1.ClientConfirmResult?, err = app.sendRequest("app1", "_client_confirm", { question = question })
  if err ~= nil then
    return ctx:fail("Nobody could confirm it: no App 1 window is open (" .. err.kind .. ")")
  end
  if not ok then
    return ctx:fail("Not cleared: the person said no")
  end
  local count = #items
  table.clear(items)
  app.sendNotification("cleared", { count = count })
  saved()
  return count
end)

app.onRequest("_items", function(): app1.ItemsResult
  return items
end)
