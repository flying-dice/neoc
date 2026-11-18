--!strict
-- The Calculator from docs/architecture/ui-system.md §4.3, verbatim apart from the type fixes noted inline:
-- `bun run lint:luau` type-checks it against the generated definitions, and the codegen contract test runs it
-- (against the real `ui.luau`) and validates the tree it renders.
local app = require("@agentos/app")
local ui = require("@agentos/ui")
local calc = require("./calc")
local t = app.t

app.manifest {
  id = "calculator",
  name = "Calculator",
  icon = "plus",
  tint = "orange",
  window = { w = 340, h = 560, minW = 320, minH = 480 }, -- below 480 tall the keys fall under 44 px
}

-- One calculator for the whole app, however many windows are open.
local s = app.state {
  entry = "0",
  acc = 0,
  hasAcc = false,
  pending = "",
  fresh = true,
  caption = "",
  err = false,
}

local SYMBOL = { ["+"] = "+", ["-"] = "−", ["*"] = "×", ["/"] = "÷" }

-- ── Logic (pure Luau; no UI in here) ──────────────────────────────────────

local function apply(a: number, op: string, b: number): number
  s.caption = `{calc.format(a)} {SYMBOL[op]} {calc.format(b)}`
  return calc.evaluate(`{a} {op} ({b})`) -- raises on division by zero
end

local function digit(d: string)
  if s.fresh then
    s.entry, s.fresh = (if d == "." then "0." else d), false
  elseif d == "." then
    if not string.find(s.entry, ".", 1, true) then
      s.entry ..= "."
    end
  elseif #string.gsub(s.entry, "[-.]", "") < 9 then
    s.entry = if s.entry == "0" then d else s.entry .. d
  end
end

local function operator(op: string)
  local value = tonumber(s.entry) :: number
  if s.hasAcc and s.pending ~= "" and not s.fresh then
    s.acc = apply(s.acc, s.pending, value)
    s.entry = tostring(s.acc)
  else
    s.acc, s.hasAcc = value, true
  end
  s.pending, s.fresh = op, true
end

local function equals()
  if not s.hasAcc or s.pending == "" then
    return
  end
  s.entry = tostring(apply(s.acc, s.pending, tonumber(s.entry) :: number))
  s.hasAcc, s.pending, s.fresh = false, "", true
end

local function clear() -- "C" clears what is being typed; "AC" (nothing typed) clears everything
  if s.entry ~= "0" and not s.fresh then
    s.entry = "0"
  else
    s.entry, s.hasAcc, s.pending, s.caption = "0", false, "", ""
  end
  s.err, s.fresh = false, true
end

local actions: { [string]: (string?) -> () } = {
  digit = digit,
  operator = operator,
  equals = equals,
  clear = clear,
  negate = function()
    s.entry = if s.entry:sub(1, 1) == "-" then s.entry:sub(2) elseif s.entry == "0" then "0" else "-" .. s.entry
  end,
  percent = function()
    s.entry = tostring((tonumber(s.entry) :: number) / 100)
  end,
  backspace = function()
    if not s.fresh then
      s.entry = if #s.entry > 1 and s.entry ~= "-0" then s.entry:sub(1, -2) else "0"
    end
  end,
}

local function press(kind: string, arg: string?)
  if s.err and kind ~= "clear" then
    return
  end
  if not pcall(actions[kind], arg) then
    s.err, s.hasAcc, s.pending, s.fresh = true, false, "", true
  end
end

-- ── View ─────────────────────────────────────────────────────────────────

local function digitKey(d: string)
  return { id = d, name = d, label = d, kind = "digit", arg = d, face = "digit", shortcut = d }
end
local function opKey(id: string, symbol: string, arg: string, name: string)
  return { id = id, name = name, label = symbol, kind = "operator", arg = arg, face = "operator", shortcut = arg }
end

-- type fix: the keys are heterogeneous tables (not every key has a shortcut or a span)
local KEYS: { any } = {
  { id = "clear", name = "Clear", label = "AC", kind = "clear", face = "function", shortcut = "escape" },
  { id = "negate", name = "Plus minus", label = "±", kind = "negate", face = "function" },
  { id = "percent", name = "Percent", label = "%", kind = "percent", face = "function", shortcut = "%" },
  opKey("div", "÷", "/", "Divided by"),
  digitKey("7"),
  digitKey("8"),
  digitKey("9"),
  opKey("mul", "×", "*", "Times"),
  digitKey("4"),
  digitKey("5"),
  digitKey("6"),
  opKey("sub", "−", "-", "Minus"),
  digitKey("1"),
  digitKey("2"),
  digitKey("3"),
  opKey("add", "+", "+", "Plus"),
  { id = "0", name = "0", label = "0", kind = "digit", arg = "0", face = "digit", shortcut = "0", span = 2 },
  { id = "dot", name = "Decimal point", label = ".", kind = "digit", arg = ".", face = "digit", shortcut = "." },
  { id = "eq", name = "Equals", label = "=", kind = "equals", face = "operator", shortcut = "enter" },
}
local FILL: { [string]: "gray4" | "gray2" | "orange" } =
  { digit = "gray4", ["function"] = "gray2", operator = "orange" } -- semantic palette, auto-contrast text

local function keyView(k: any)
  local title = k.label
  if k.id == "clear" and s.entry ~= "0" and not s.fresh then
    title = "C"
  end
  return ui.Button {
    key = "key-" .. k.id,
    title = title,
    label = k.name,
    shape = "circle",
    size = "fill",
    fill = FILL[k.face],
    span = k.span,
    selected = k.kind == "operator" and s.pending == k.arg and s.fresh, -- aria-pressed on the pending operator
    shortcut = k.shortcut,
    onTap = function()
      press(k.kind, k.arg)
    end,
  }
end

app.window("main", {
  title = "Calculator",
  render = function(w)
    local display = if s.err then "Error" else calc.formatEntry(s.entry)
    return ui.Scope {
      scheme = "dark",
      background = "primary", -- the instrument canvas: black body
      ui.VStack {
        spacing = 2,
        padding = 3,
        ui.Spacer {},
        ui.Text {
          key = "caption",
          s.caption,
          style = "body",
          foreground = "secondary",
          align = "trailing",
          lines = 1,
        },
        ui.Text {
          key = "display",
          display,
          label = "Result",
          style = "display",
          fit = true,
          align = "trailing",
          lines = 1,
          live = "polite",
        },
        ui.Grid { columns = 4, spacing = 2, cellAspect = 1, ui.each(KEYS, keyView) },
        ui.Shortcut {
          key = "backspace",
          shortcut = "backspace",
          onTrigger = function()
            press("backspace")
          end,
        },
      },
    }
  end,
})

-- ── Agent tools ──────────────────────────────────────────────────────────

local function record(expression: string, result: number)
  local h = app.kv:get("history") or {}
  table.insert(h, 1, { expression = expression, result = result })
  while #h > 50 do
    table.remove(h)
  end
  app.kv:set("history", h)
end

app.tool {
  name = "calculate",
  title = "Calculate",
  readOnly = true,
  description = "Evaluate an arithmetic expression exactly. Supports + - * / % ^, parentheses, unary minus and decimals.",
  input = { expression = t.string { description = 'Arithmetic expression, e.g. "(12.5 + 7) * 3"' } },
  run = function(input)
    -- readOnly: no history entry here (a readOnly tool cannot write; `show_on_calculator` records)
    local result = calc.evaluate(input.expression)
    return { expression = input.expression, result = result, display = calc.format(result) }
  end,
}

app.tool {
  name = "show_on_calculator",
  title = "Show on Calculator",
  description = "Open the Calculator and show the result of an expression, so the user can keep calculating.",
  input = { expression = t.string {} },
  run = function(input)
    local result = calc.evaluate(input.expression)
    record(input.expression, result)
    s.entry, s.caption, s.hasAcc, s.pending, s.fresh, s.err = tostring(result), input.expression, false, "", true, false
    app:openWindow("main") -- every open Calculator window re-renders by itself
    return { shown = calc.format(result) }
  end,
}

app.tool {
  name = "history",
  title = "Calculation History",
  readOnly = true,
  description = "List recent calculations, newest first.",
  input = { limit = t.integer { default = 10, min = 1, max = 50 } },
  run = function(input)
    local h = app.kv:get("history") or {}
    return { entries = table.move(h, 1, math.min(#h, input.limit), 1, {}) }
  end,
}
