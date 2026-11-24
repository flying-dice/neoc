--!strict
-- lib:log as a script sees it: its answers (log.json), then what it asserts. A script cannot see what was written (the
-- Rust tests read the lines through a sink), so this is which calls are accepted; every logger here is silent, or the
-- call is below the default level.
local log = require("lib:log")
local out = {}

-- whether the call is refused (the words of a refusal differ)
local function refused(f, ...)
  return not pcall(f, ...)
end

out.module = {
  type(log.new),
  type(log.trace),
  type(log.debug),
  type(log.info),
  type(log.warn),
  type(log.error),
}

local logger = log.new { level = "off" }
out.logger = { type(logger), type(logger.info), type(logger.with) }
out.new = {
  (log.new() :: any) ~= nil,
  (log.new {} :: any) ~= nil,
  (log.new { level = "error" } :: any) ~= nil,
  (log.new { level = "off", format = "text" } :: any) ~= nil,
  (log.new { format = "json" } :: any) ~= nil,
}
out.new_refused = {
  refused(log.new, { level = "verbose" }),
  refused(log.new, { format = "yaml" }),
  refused(log.new, { level = 5 }),
}

-- every level, with and without fields; a call returns nothing
out.returns = {
  select("#", logger:trace("hidden")),
  select("#", logger:debug("hidden")),
  select("#", logger:info("hidden", { method = "GET", status = 200 })),
  select("#", logger:warn("hidden", {})),
  select("#", logger:error("hidden")),
  select("#", log.trace("below the default level")),
  select("#", log.debug("below the default level", { k = "v" })),
}

-- with: a child, and the parent still works
local child = logger:with { request_id = "abc" }
local grandchild = child:with { b = 2 }
grandchild:info("chained")
child:info("ok")
logger:info("x")
out.with = { type(child), type(grandchild), child ~= logger } :: { any }

-- what a field may hold
local circular = {}
circular.self = circular
logger:info("msg", { present = "yes", absent = nil })
logger:info("msg", { cb = function() end })
logger:info("msg", { user = { id = 1, name = "alice" } })
logger:info("msg", { l1 = { l2 = { l3 = { l4 = { l5 = "x" } } } } })
logger:info("msg", { node = circular })
logger:info("msg", { items = { 1, 2, 3 } })
logger:info("msg", { str = "hello", num = 3.14, int = 42, bool = true, empty = nil, nested = { ok = true } })
logger:info("msg", { nested = { [true] = "x" } })
logger:info("msg", { level = "shadow", ts = "FAKE", msg = "FAKEMSG", user = "alice" })
out.fields_refused = { refused(logger.info, logger, "msg", 5), refused(logger.info, logger, "msg", "text") }
out.message_refused = { refused(logger.info, logger), refused(logger.info, logger, {}) }

-- what it asserts
do
  local logger = log.new { level = "off", format = "text" }
  logger:info("hidden", { method = "GET", status = 200, nested = { ok = true }, cb = function() end })
  logger:with({ request_id = "abc" }):with({ b = 2 }):error("hidden")
  log.debug("below the default level", { k = "v" })

  -- options: the words as written, and only the options there are
  -- deliberate refusal test: none is the options
  for _, options in
    {
      { level = "verbose" },
      { level = "INFO" },
      { format = "yaml" },
      { format = "TEXT" },
      { level = 5 },
      { levle = "info" },
    } :: { any }
  do
    local ok, refused = pcall(log.new, options)
    assert(not ok and tostring(refused):find("lib:log.new: argument #1 'options'", 1, true), tostring(refused))
  end

  -- fields: a table, keyed by names
  -- deliberate refusal test: none is fields
  for _, fields in { 5, "text", { [true] = "v" }, { "a", "b" } } :: { any } do
    local ok, refused = pcall(function(): ...unknown
      logger:info("msg", fields)
    end)
    assert(not ok and tostring(refused):find("lib:log.Logger:info: argument #2 'fields'", 1, true), tostring(refused))
    local ok_with, refused_with = pcall(logger.with, logger, fields)
    assert(not ok_with and tostring(refused_with):find("lib:log.Logger:with: argument #1 'fields'", 1, true))
    local ok_module, refused_module = pcall(function(): ...unknown
      log.trace("msg", fields)
    end)
    assert(not ok_module and tostring(refused_module):find("lib:log.trace: argument #2 'fields'", 1, true))
  end

  -- a key that is not a string is still fine below the top
  local odd_keys: { [boolean | number]: string } = { [true] = "x", [1.5] = "y" }
  logger:info("msg", { nested = odd_keys })

  -- the message is text
  local ok, refused = pcall(function(): ...unknown
    logger:info(5 :: any) -- deliberate refusal test: a number is not a message
  end)
  assert(not ok and tostring(refused):find("lib:log.Logger:info: argument #1 'message'", 1, true))
end

return out
