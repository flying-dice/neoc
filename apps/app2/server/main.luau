--!strict
-- App 2's backend talks to App 1's on the bus, through the global `app` (nothing generated runs: `types/app2` and
-- `types/app1` are types). It hears App 1's public notifications and sends App 1 requests; what it hears it keeps,
-- and tells App 2's page with its own private notification, `_overheard`.
local app2 = require("../types/app2")
local app1 = require("../types/app1")

local heard: { string } = {}

local function hear(line: string)
  table.insert(heard, 1, line)
  if #heard > 50 then
    table.remove(heard)
  end
  app.sendNotification("_overheard", { line = line })
end

-- `ctx.actor` is the app that sent the notification: App 1
app.onNotification("app1", "added", function(params: app1.AddedParams, ctx: app2.Context)
  local item = params.item
  hear(ctx.actor.id .. " added #" .. tostring(item.id) .. " " .. item.text .. " (by " .. item.by .. ")")
end)
app.onNotification("app1", "cleared", function(params: app1.ClearedParams, ctx: app2.Context)
  hear(ctx.actor.id .. " cleared " .. tostring(params.count))
end)

app.onRequest("_relay", function(params: app2.RelayParams, ctx: app2.Context): app2.RelayResult
  local item: app1.AddResult?, err = app.sendRequest("app1", "add", { text = params.text })
  if err ~= nil or item == nil then
    return ctx:fail("App 1 did not add it (" .. (if err then err.kind else "no answer") .. ")")
  end
  return { id = item.id, by = item.by }
end)

app.onRequest("_heard", function(): app2.HeardResult
  return heard
end)
