--!strict
-- A typed Luau app for the host's own test (`typed_luau.rs`): type aliases, annotations, a generic function, a cast, a
-- typed `require` of a neoc module and the Luau syntax the host runs (`continue`, `+=`, string interpolation, an
-- `if` expression, generalized iteration). `bun run lint:luau` holds it to strict mode like every app.
local app = require("vos:app")
local ui = require("vos:ui")
local base64 = require("lib:base64")

type Stats = { count: number, total: number }

app.manifest {
  id = "typed",
  name = "Typed",
  icon = "puzzlepiece",
  tint = "blue",
  window = { w = 320, h = 240, minW = 320, minH = 240 },
}

-- the non-negative numbers, counted and added up
local function summarize(numbers: { number }): Stats
  local stats: Stats = { count = 0, total = 0 }
  for _, number in numbers do
    if number < 0 then
      continue
    end
    stats.count += 1
    stats.total += number
  end
  return stats
end

local function describe<T>(label: string, value: T): string
  return `{label}: {value}`
end

app.tool {
  name = "summarize",
  title = "Summarize",
  readOnly = true,
  input = { numbers = app.t.array(app.t.number {}) },
  run = function(input: { numbers: { number } })
    local stats = summarize(input.numbers)
    local size = if stats.count > 2 then "many" else "few"
    return {
      count = stats.count,
      total = stats.total,
      text = describe("sum", stats.total),
      size = size,
      encoded = base64.encode(size :: string),
    }
  end,
}

app.window("main", {
  title = "Typed",
  render = function(w: vos_WindowContext): vos_Node
    return ui.Text { key = "hello", `Hello from window {w.id}`, style = "title1" }
  end,
})
