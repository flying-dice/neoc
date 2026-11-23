-- lib:log on neoc-vm and neoc-vm2: what both must answer the same. A script cannot see what was written (the Rust
-- tests read the lines through a sink), so this is which calls are accepted; every logger here is silent, or the
-- call is below the default level.
local log = require("lib:log")
local out = {}

-- whether the call is refused (the words of a refusal differ)
local function refused(f, ...)
	return not pcall(f, ...)
end

out.module = {
	type(log.new), type(log.trace), type(log.debug), type(log.info), type(log.warn), type(log.error),
}

local logger = log.new({ level = "off" })
out.logger = { type(logger), type(logger.info), type(logger.with) }
out.new = {
	log.new() ~= nil,
	log.new({}) ~= nil,
	log.new({ level = "error" }) ~= nil,
	log.new({ level = "off", format = "text" }) ~= nil,
	log.new({ format = "json" }) ~= nil,
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
local child = logger:with({ request_id = "abc" })
local grandchild = child:with({ b = 2 })
grandchild:info("chained")
child:info("ok")
logger:info("x")
out.with = { type(child), type(grandchild), child ~= logger }

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

return out
