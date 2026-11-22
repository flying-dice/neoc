--!strict
-- The Calculator's backend. The keypad keeps nothing here: the client holds its state and sends it with each key, and
-- `press` answers with the next state and what to draw (keypad.luau, calc.luau). What the backend does keep is what an
-- agent did: the history of `calculate` and `show_on_calculator`, and a result shown on the Calculator that no page has
-- taken yet.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local calc = require("./calc")
local keypad = require("./keypad")
local t = app.t

-- The history: newest first, at most 50, in app.kv under "history" as { expression, result }. The same key and shape the
-- Calculator has always used, so a host that ran the earlier Calculator keeps its history.
local HISTORY = "history"
-- A result an agent put on the Calculator that no page has shown yet: { expression, result }. One at a time: a second
-- show replaces the first.
local SHOWN = "shown"
-- The event that tells an open page a result is waiting for it.
local SHOWN_EVENT = "calculator.shown"

type Entry = { expression: string, result: number }

local function record(expression: string, result: number)
  local h: { Entry } = app.kv:get(HISTORY) or {}
  table.insert(h, 1, { expression = expression, result = result })
  while #h > 50 do
    table.remove(h)
  end
  app.kv:set(HISTORY, h)
end

local handlers: rpc.Handlers = {
  press = function(_ctx, args)
    return keypad.press(args.state or keypad.initial(), args.key)
  end,
  -- The page asks when it starts and when SHOWN_EVENT arrives: the waiting result, once.
  takeShown = function(_ctx)
    local waiting: Entry? = app.kv:get(SHOWN)
    if waiting == nil then
      return {}
    end
    app.kv:delete(SHOWN)
    return { screen = keypad.shown(waiting.result, waiting.expression) }
  end,
}

app.tool {
  name = "calculate",
  title = "Calculate",
  -- not readOnly: it writes the history
  description = "Evaluate an arithmetic expression with the usual precedence (2 + 3 * 4 is 14), rounded to 12 significant digits. Supports + - * / % ^, parentheses, unary minus and decimals.",
  input = { expression = t.string { description = 'Arithmetic expression, e.g. "(12.5 + 7) * 3"' } },
  run = function(input: { expression: string })
    local result = calc.evaluate(input.expression)
    record(input.expression, result)
    return { expression = input.expression, result = result, display = calc.format(result) }
  end,
}

app.tool {
  name = "show_on_calculator",
  title = "Show on Calculator",
  description = "Work out an expression and put the result on the person's Calculator, so they can keep calculating from it. "
    .. "vos opens the Calculator (or brings it forward) on every connected screen, and the result replaces whatever sum "
    .. "was on it. If no screen is connected, the result waits and appears the next time the Calculator is opened; a "
    .. "second call before then replaces the first.",
  input = { expression = t.string { description = 'Arithmetic expression, e.g. "18 / 100 * 2450"' } },
  run = function(input: { expression: string })
    local result = calc.evaluate(input.expression)
    record(input.expression, result)
    app.kv:set(SHOWN, { expression = input.expression, result = result })
    app.emit(SHOWN_EVENT)
    app:openWindow()
    return {
      shown = calc.format(result),
      outcome = "The Calculator was asked to open on every connected screen and show this result; if none is connected, it appears the next time the Calculator is opened.",
    }
  end,
}

app.tool {
  name = "history",
  title = "Calculation History",
  readOnly = true,
  description = "List recent calculations (from calculate and show_on_calculator), newest first.",
  input = { limit = t.integer { default = 10, min = 1, max = 50 } },
  run = function(input: { limit: number })
    local h: { Entry } = app.kv:get(HISTORY) or {}
    local entries = {}
    for i = 1, math.min(#h, input.limit) do
      entries[i] = h[i]
    end
    return { entries = entries }
  end,
}

return handlers
