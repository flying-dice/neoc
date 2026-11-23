-- lib:compile on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- A successful bundle copies the running binary (hundreds of megabytes for a test binary), so only the failures run
-- here; the bundle itself is covered by the module's Rust tests. What an error is differs on purpose: see
-- compile.vm2.luau.
local compile = require("lib:compile")
local path = require("std:path")
local env = require("std:env")
local fs = require("std:fs")
local random = require("lib:random")

local out = {}

local dir = path.join(env.temp_dir(), "neoc-conformance-compile-" .. random.uuid_v4())
local output = path.join(dir, "app")
local value, err = compile.bundle({ entrypoint = path.join(dir, "nope.lua"), output = output })
-- a failure is `(nil, err)`, and nothing was written
out.missing_entrypoint = { value == nil, err ~= nil, fs.exists(output) }

-- both fields are required
out.wrong_arguments = {
	(pcall(compile.bundle, { output = output })),
	(pcall(compile.bundle, { entrypoint = "main.lua" })),
	(pcall(compile.bundle, { entrypoint = "main.lua", output = {} })),
	(pcall(compile.bundle)),
}

return out
