local thread = require("std:thread")

-- Worker script used by tests/std/thread.test.lua to exercise the `file` option.
-- Args arrive as the first vararg (Node `workerData` style); `thread.parent`
-- is the bidirectional channel back to the spawner.

local args   = ...
local parent = thread.parent

if parent then
    parent:send("hello from file worker")
    local cmd = parent:receive()
    if cmd == "ack" then
        parent:send("acknowledged")
    end
end

return args.n * args.n
