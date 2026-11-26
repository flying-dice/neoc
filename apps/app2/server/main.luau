--!strict
-- App 2's backend talks to App 1's on the bus, through the global `RPC`, typed by the generated `types/vos.d.luau`
-- (nothing in it runs, and nothing is required). It hears App 1's public notifications and sends App 1 requests; what
-- it hears it keeps, and tells App 2's page with its own private notification, `_overheard`.

local heard: { string } = {}

local function hear(line: string)
  table.insert(heard, 1, line)
  if #heard > 50 then
    table.remove(heard)
  end
  RPC.send_notification("_overheard", { line = line })
end

-- `ctx.actor` is the app that sent the notification: App 1
RPC.on_notification("app1", "added", function(params: app1_AddedParams, ctx: vos_app_Context)
  local item = params.item
  hear(ctx.actor.id .. " added #" .. tostring(item.id) .. " " .. item.text .. " (by " .. item.by .. ")")
end)
RPC.on_notification("app1", "cleared", function(params: app1_ClearedParams, ctx: vos_app_Context)
  hear(ctx.actor.id .. " cleared " .. tostring(params.count))
end)

RPC.on_request("_relay", function(params: app2_RelayParams, ctx: vos_app_Context): app2_RelayResult
  local item: app1_AddResult?, err = RPC.send_request("app1", "add", { text = params.text })
  if err ~= nil or item == nil then
    return ctx:fail("App 1 did not add it (" .. (if err then err.kind else "no answer") .. ")")
  end
  return { id = item.id, by = item.by }
end)

RPC.on_request("_heard", function(): app2_HeardResult
  return heard
end)
