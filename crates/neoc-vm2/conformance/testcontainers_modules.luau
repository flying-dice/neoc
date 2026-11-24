-- vnd:testcontainers_modules on neoc-vm and neoc-vm2: what both answer the same without a Docker daemon. Starting a
-- container needs one, so that is left to the module's own tests (ignored unless asked for); here are the module's
-- three functions and that a container nobody could start is not started. (neoc-vm returns the refusal, neoc-vm2
-- raises it as a wrong argument: testcontainers_modules.vm2.luau.)
local testcontainers_modules = require("vnd:testcontainers_modules") :: any
local out = {}

out.functions = {
	type(testcontainers_modules.mysql),
	type(testcontainers_modules.postgres),
	type(testcontainers_modules.generic),
}

local function refused(image: any): boolean
	local ok, container = pcall(testcontainers_modules.generic, image)
	return not ok or container == nil
end
out.no_image = refused({})
out.unknown_wait = refused({ image = "alpine", wait = { type = "nonsense" } })

return out
