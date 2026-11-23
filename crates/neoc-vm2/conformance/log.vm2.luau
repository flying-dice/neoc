-- intentional: level and format are string enums exactly as written ("info", not "INFO") and an option that is not one, or an option that does not exist, is refused as an argument with its place; fields are a table with string keys, so a key that is not a string is refused as an argument (it was written as text or as <invalid key>).
local log = require("lib:log")

local logger = log.new({ level = "off", format = "text" })
logger:info("hidden", { method = "GET", status = 200, nested = { ok = true }, cb = function() end })
logger:with({ request_id = "abc" }):with({ b = 2 }):error("hidden")
log.debug("below the default level", { k = "v" })

-- options: the words as written, and only the options there are
for _, options in { { level = "verbose" }, { level = "INFO" }, { format = "yaml" }, { format = "TEXT" }, { level = 5 }, { levle = "info" } } :: { any } do
	local ok, refused = pcall(log.new, options)
	assert(not ok and tostring(refused):find("lib:log.new: argument #1 'options'", 1, true), tostring(refused))
end

-- fields: a table, keyed by names
for _, fields in { 5, "text", { [true] = "v" }, { "a", "b" } } :: { any } do
	local ok, refused = pcall(logger.info, logger, "msg", fields)
	assert(not ok and tostring(refused):find("lib:log.Logger:info: argument #2 'fields'", 1, true), tostring(refused))
	local ok_with, refused_with = pcall(logger.with, logger, fields)
	assert(not ok_with and tostring(refused_with):find("lib:log.Logger:with: argument #1 'fields'", 1, true))
	local ok_module, refused_module = pcall(log.trace, "msg", fields)
	assert(not ok_module and tostring(refused_module):find("lib:log.trace: argument #2 'fields'", 1, true))
end

-- a key that is not a string is still fine below the top
logger:info("msg", { nested = { [true] = "x", [1.5] = "y" } })

-- the message is text
local ok, refused = pcall(logger.info, logger, 5 :: any)
assert(not ok and tostring(refused):find("lib:log.Logger:info: argument #1 'message'", 1, true))
