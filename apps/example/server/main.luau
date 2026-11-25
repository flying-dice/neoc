--!strict
-- The Example app's backend: the smallest whole one. Each method of openrpc.json has a handler here, typed by the
-- router the build generates from the contract (`.app/rpc`): the params arrive checked against the contract's schemas,
-- and what a handler returns must match the method's result. A refusal the person should read is `ctx:fail(message)`
-- (code -32010); the page shows the message. `require("vos:app")` is where an app's database (`app.db`) and the
-- tools it gives agents (`app.tool`) are: the Example needs neither yet.
local rpc = require("../.app/rpc")
local parts = require("./parts")
local trim = require("lib:text").trim

local handlers: rpc.Handlers = {
  parts = function(_ctx)
    return parts.all()
  end,

  greet = function(ctx, params)
    local name = trim(params.name)
    if name == "" then
      return ctx:fail("Type a name to greet.")
    end
    return { message = `Hello, {name}! This came from the Example app's Luau backend.` }
  end,
}

return handlers
