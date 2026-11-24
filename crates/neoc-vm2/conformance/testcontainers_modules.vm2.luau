--!strict
-- intentional: vnd:testcontainers_modules returns a typed ContainerError (docker | not_ready | not_exposed | other), not a text; generic takes a typed table, so a missing image, an unknown key or an unknown wait type is refused as a wrong argument with its place; wait type "log" is gone (it was "stdout"); stop returns true or nil and the error
local testcontainers_modules = require("vnd:testcontainers_modules")

-- what cannot be started is refused before Docker is asked: no daemon is needed for any of this
local function refusal(image: unknown): string
	local ok, err = pcall(function()
		testcontainers_modules.generic(image :: any)
	end)
	assert(not ok, "accepted")
	return tostring(err)
end

local place = "vnd:testcontainers_modules.generic: argument #1 'image'"
assert(refusal({}):find(place, 1, true))
assert(refusal("alpine"):find(place, 1, true))
assert(refusal({ image = "alpine", tga = "3.19" }):find("unknown field `tga`", 1, true))
assert(refusal({ image = "alpine", ports = { 70000 } }):find("ports[1]", 1, true))
assert(refusal({ image = "alpine", wait = { type = "nonsense" } }):find("`type` must be one of", 1, true))
assert(refusal({ image = "alpine", wait = { type = "log", message = "ready" } }):find("`type` must be one of", 1, true))
assert(refusal({ image = "alpine", wait = { type = "seconds" } }):find(place, 1, true))

-- what a script writes type-checks; it is not run here, because it needs Docker
local function _usage()
	local nginx, err = testcontainers_modules.generic({
		image = "nginx",
		tag = "1.27-alpine",
		ports = { 80 },
		env = { MODE = "test" },
		cmd = { "nginx", "-g", "daemon off;" },
		wait = { type = "stderr", message = "start worker processes" },
	})
	if nginx == nil then
		assert(err ~= nil)
		if err.kind == "not_exposed" then
			local _port: number = err.port
		elseif err.kind == "docker" or err.kind == "not_ready" or err.kind == "other" then
			local _reason: string = err.reason
		end
		return
	end
	local _address: string = nginx:host() .. ":" .. nginx:port(80)
	local stopped, stop_err = nginx:stop()
	assert(stopped == true and stop_err == nil)

	local mysql = assert(testcontainers_modules.mysql())
	local _url: string = mysql:url()
	local _port: number = mysql:port()
	assert(mysql:stop())
	local postgres = assert(testcontainers_modules.postgres())
	local _pg: string = postgres:url() .. postgres:host()
	assert(postgres:stop())
	testcontainers_modules.generic({ image = "alpine", wait = { type = "stdout", message = "ready" } })
	testcontainers_modules.generic({ image = "alpine", wait = { type = "seconds", seconds = 2 } })
	testcontainers_modules.generic({ image = "alpine", wait = { type = "healthcheck" } })
end

-- the module cannot be patched
assert(not pcall(function()
	(testcontainers_modules :: any).mysql = nil
end))

return true
