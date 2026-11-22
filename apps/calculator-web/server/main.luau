--!strict
-- The Calculator's backend. It keeps nothing: the client holds the keypad's state and sends it with each key, and `press`
-- answers with the next state and what to draw. The arithmetic and the keypad's rules are keypad.luau and calc.luau,
-- the same rules as the old Calculator (apps/calculator), which this app replaces once it does everything that one does.
local rpc = require("../.app/rpc")
local keypad = require("./keypad")

local handlers: rpc.Handlers = {
  press = function(_ctx, args)
    return keypad.press(args.state or keypad.initial(), args.key)
  end,
}

return handlers
