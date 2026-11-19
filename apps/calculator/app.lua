-- The Calculator: a keypad window and three agent tools. `bun run build:apps` packages it (with app.json) into
-- an app ref.
local app = require("@agentos/app")
local ui = require("@agentos/ui")
local calc = require("./calc")
local t = app.t

app.manifest {
  id = "calculator",
  name = "Calculator",
  icon = "calculator",
  tint = "orange",
  window = { w = 340, h = 560, minW = 320, minH = 500 }, -- below 500 tall the keypad no longer fits
}

-- One calculator for the whole app, however many windows are open.
-- `entry` is the text being typed; once a result is in (`fresh`) the number lives in `value`, so a result is never
-- parsed back from its own display text.
local s = app.state {
  entry = "0",
  value = 0,
  acc = 0,
  hasAcc = false,
  pending = "",
  fresh = true,
  caption = "",
  err = false,
}

local SYMBOL = { ["+"] = "+", ["-"] = "−", ["*"] = "×", ["/"] = "÷" }

-- ── Logic (pure Lua; no UI in here) ──────────────────────────────────────

local function apply(a, op, b)
  s.caption = calc.format(a) .. " " .. SYMBOL[op] .. " " .. calc.format(b)
  return calc.combine(a, op, b) -- raises on division by zero
end

-- The number on the display: the result once one is in, else what has been typed.
local function current()
  if s.fresh then
    return s.value
  end
  return tonumber(s.entry)
end

-- Show a number (a result, or a value an agent put there); the next digit starts a new entry.
local function show(value)
  s.value, s.fresh = value, true
end

local function digit(d)
  if s.fresh then
    s.entry, s.fresh = (d == "." and "0." or d), false
  elseif d == "." then
    if not string.find(s.entry, ".", 1, true) then
      s.entry = s.entry .. "."
    end
  elseif #string.gsub(s.entry, "[-.]", "") < 9 then
    if s.entry == "0" then
      s.entry = d
    else
      s.entry = s.entry .. d
    end
  end
end

local function operator(op)
  local value = current()
  if s.hasAcc and s.pending ~= "" and not s.fresh then
    s.acc = apply(s.acc, s.pending, value)
    show(s.acc)
  else
    s.acc, s.hasAcc = value, true
    show(value)
  end
  s.pending = op
  s.caption = calc.format(s.acc) .. " " .. SYMBOL[op] -- "27 −": what is waiting, in words as well as on the key
end

local function equals()
  if not s.hasAcc or s.pending == "" then
    return
  end
  show(apply(s.acc, s.pending, current()))
  s.hasAcc, s.pending = false, ""
end

local function clear() -- "C" clears what is being typed; "AC" (nothing typed) clears everything
  if s.entry ~= "0" and not s.fresh then
    s.entry = "0"
  else
    s.entry, s.hasAcc, s.pending, s.caption = "0", false, "", ""
  end
  s.err, s.value, s.fresh = false, 0, true
end

local actions = {
  digit = digit,
  operator = operator,
  equals = equals,
  clear = clear,
  negate = function()
    if s.fresh then
      if s.value == 0 then
        s.value = 0 -- never "-0"
      else
        s.value = -s.value
      end
    elseif s.entry:sub(1, 1) == "-" then
      s.entry = s.entry:sub(2)
    elseif s.entry ~= "0" then
      s.entry = "-" .. s.entry
    end
  end,
  percent = function()
    show(calc.combine(current(), "/", 100))
  end,
  backspace = function()
    if not s.fresh then
      local rest = s.entry:sub(1, -2)
      if rest == "" or rest == "-" or rest == "-0" then
        s.entry = "0"
      else
        s.entry = rest
      end
    end
  end,
}

-- Every key goes through here, with the window it was pressed in. A calculation that fails (calc.lua raises a plain
-- sentence) leaves the display at "Error" with the reason under it, and the window announces the reason: the
-- announcer is always on the page, so a screen reader hears it (a live region that appears together with its text is
-- often missed). "AC" gets out of it, and so does a digit, which starts a new sum as it does on an iPad; the other
-- keys have nothing to work on and are ignored.
local function press(w, kind, arg)
  local wasErr = s.err
  if s.err then
    if kind == "digit" then
      actions.clear()
    elseif kind ~= "clear" then
      return
    end
  end
  local ok, err = pcall(actions[kind], arg)
  if not ok then
    local reason = calc.reason(err) -- an unexpected error is a generic sentence, never a source position
    s.err, s.hasAcc, s.pending, s.fresh = true, false, "", true
    s.caption = reason
    w:announce(reason)
  elseif wasErr and not s.err then
    w:announce("") -- the announcer would otherwise keep the old reason, and not repeat it if the same error comes back
  end
end

-- ── View ─────────────────────────────────────────────────────────────────

local function digitKey(d)
  return { id = d, name = d, label = d, kind = "digit", arg = d, face = "digit", shortcut = d }
end
local function opKey(id, symbol, arg, name)
  return { id = id, name = name, label = symbol, kind = "operator", arg = arg, face = "operator", shortcut = arg }
end

-- the keys are heterogeneous tables (not every key has a shortcut or a span)
local KEYS = {
  { id = "clear", name = "Clear", label = "AC", kind = "clear", face = "function", shortcut = "escape" },
  { id = "negate", name = "Change sign", label = "±", kind = "negate", face = "function" },
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
  {
    id = "0",
    name = "0",
    label = "0",
    kind = "digit",
    arg = "0",
    face = "digit",
    shortcut = "0",
    span = 2,
    wide = true,
  },
  { id = "dot", name = "Decimal point", label = ".", kind = "digit", arg = ".", face = "digit", shortcut = "." },
  { id = "eq", name = "Equals", label = "=", kind = "equals", face = "operator", shortcut = "enter" },
}
local FILL = { digit = "gray4", ["function"] = "gray2", operator = "orange" } -- semantic palette, auto-contrast text

-- iPadOS sets the wide "0" with its label over the first circle, not centred in the capsule
local function wideLabel(k)
  return ui.HStack {
    key = "label",
    frame = "fill",
    align = "center",
    padding = { left = 7 },
    ui.Text { key = "text", k.label, style = "headline" },
    ui.Spacer { key = "rest" },
  }
end

local function keyView(k, w)
  local title, name = k.label, k.name
  if k.id == "clear" and s.entry ~= "0" and not s.fresh then
    title, name = "C", "C, clear entry" -- the accessible name keeps the visible text (WCAG 2.5.3)
  elseif k.id == "clear" then
    name = "AC, all clear"
  end
  return ui.Button {
    key = "key-" .. k.id,
    title = not k.wide and title or nil,
    label = name,
    shape = "circle",
    size = "fill",
    fill = FILL[k.face],
    span = k.span,
    selected = k.kind == "operator" and s.pending == k.arg and s.fresh, -- aria-pressed on the pending operator
    shortcut = k.shortcut,
    onTap = function()
      press(w, k.kind, k.arg)
    end,
    k.wide and { wideLabel(k) } or {},
  }
end

app.window("main", {
  title = "Calculator",
  render = function(w)
    local display
    if s.err then
      display = "Error"
    elseif s.fresh then
      display = calc.format(s.value)
    else
      display = calc.formatEntry(s.entry)
    end
    return ui.Scope {
      scheme = "dark",
      background = "primary", -- the instrument canvas: black body
      frame = "fill",
      ui.VStack {
        frame = "fill",
        align = "center",
        spacing = 2,
        padding = 3,
        ui.Spacer {},
        -- one column, a fixed 296 pt wide (the narrowest window's content) and centred: the display's right edge is
        -- the keypad's right edge, and the keypad is the same size in any window and always fits. Round keys:
        -- square cells.
        ui.VStack {
          frame = { idealW = 296, maxW = 296 },
          spacing = 2,
          ui.Text {
            key = "caption",
            s.caption,
            style = "body",
            foreground = "secondary",
            align = "trailing",
            lines = 1, -- not a live region: it changes on every sum, and the reason for an error is announced by `press`
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
          ui.Grid {
            columns = 4,
            spacing = 2,
            cellAspect = 1,
            ui.each(KEYS, function(k)
              return keyView(k, w)
            end),
          },
        },
        -- keys a Button can't carry: it has one shortcut ("enter" for =), and Backspace has no key on the pad
        ui.Shortcut {
          key = "equals",
          shortcut = "=",
          onTrigger = function()
            press(w, "equals")
          end,
        },
        ui.Shortcut {
          key = "backspace",
          shortcut = "backspace",
          onTrigger = function()
            press(w, "backspace")
          end,
        },
      },
    }
  end,
})

-- ── Agent tools ──────────────────────────────────────────────────────────

local function record(expression, result)
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
  -- not readOnly: it writes the history to app.kv
  description = "Evaluate an arithmetic expression with the usual precedence (2 + 3 * 4 is 14), rounded to 12 significant digits. Supports + - * / % ^, parentheses, unary minus and decimals.",
  input = { expression = t.string { description = 'Arithmetic expression, e.g. "(12.5 + 7) * 3"' } },
  run = function(input)
    local result = calc.evaluate(input.expression)
    record(input.expression, result)
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
    show(result)
    s.caption, s.hasAcc, s.pending, s.err = input.expression, false, "", false
    for _, w in ipairs(app:windows("main")) do
      w:announce("") -- the error that was showing is gone: empty the announcers, or the same error would not be read again
    end
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
    local entries = {}
    for i = 1, math.min(#h, input.limit) do
      entries[i] = h[i]
    end
    return { entries = entries }
  end,
}
