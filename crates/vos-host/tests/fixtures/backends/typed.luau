--!strict
-- A typed Luau backend for the host's own test (`typed_luau.rs`): type aliases, annotations, a generic function, a
-- cast, a typed `require` of a neoc module and the Luau syntax the host runs (`continue`, `+=`, string interpolation,
-- an `if` expression, generalized iteration). `bun run lint:luau` holds it to strict mode like every app.
local RPC = require("vos:rpc")
local base64 = require("lib:base64")

type Stats = { count: number, total: number }

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

RPC.on_request("summarize", function(input: { numbers: { number } })
  local stats = summarize(input.numbers)
  local size = if stats.count > 2 then "many" else "few"
  return {
    count = stats.count,
    total = stats.total,
    text = describe("sum", stats.total),
    size = size,
    encoded = base64.encode(size :: string),
  }
end, {
  title = "Summarize",
  read_only = true,
  input_schema = {
    type = "object",
    properties = { numbers = { type = "array", items = { type = "number" } } },
    required = { "numbers" },
  },
})

-- what the page calls: the context is typed by the declarations
RPC.on_request("_greet", function(args: { name: string }, ctx: vos_app_Context)
  return `Hello {args.name} from a {ctx.actor.kind}`
end)
