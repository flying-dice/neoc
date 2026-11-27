--!strict
-- The Calculator's backend. It keeps nothing: the client holds the keypad's state and sends it with each key, and `_press`
-- answers with the next state and what to draw (keypad.luau, calc.luau). It serves on the RPC bus through the global
-- `RPC`, typed by the generated `types/vos.d.luau`. `_press` is the page's method (private); an agent can do what a
-- person can do on the keypad: work a sum out (`calculate`, the same evaluator).
local calc = require("./calc")
local keypad = require("./keypad")

RPC.on_request("_press", function(params: calculator_PressParams): calculator_PressResult
  return keypad.press(params.state or keypad.initial(), params.key)
end)

RPC.on_request("calculate", function(params: calculator_CalculateParams): calculator_CalculateResult
  local result = calc.evaluate(params.expression)
  return { expression = params.expression, result = result, display = calc.format(result) }
end)
