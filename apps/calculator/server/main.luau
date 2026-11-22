--!strict
-- The Calculator's backend. It keeps nothing: the client holds the keypad's state and sends it with each key, and `press`
-- answers with the next state and what to draw (keypad.luau, calc.luau). An agent can do what a person can do on the
-- keypad: work a sum out (`calculate`, the same evaluator).
local app = require("vos:app")
local rpc = require("../.app/rpc")
local calc = require("./calc")
local keypad = require("./keypad")
local t = app.t

local handlers: rpc.Handlers = {
  press = function(_ctx, args)
    return keypad.press(args.state or keypad.initial(), args.key)
  end,
}

app.tool {
  name = "calculate",
  title = "Calculate",
  readOnly = true,
  description = "Evaluate an arithmetic expression with the usual precedence (2 + 3 * 4 is 14), rounded to 12 significant digits. Supports + - * / % ^, parentheses, unary minus and decimals.",
  input = { expression = t.string { description = 'Arithmetic expression, e.g. "(12.5 + 7) * 3"' } },
  run = function(input: { expression: string })
    local result = calc.evaluate(input.expression)
    return { expression = input.expression, result = result, display = calc.format(result) }
  end,
}

return handlers
