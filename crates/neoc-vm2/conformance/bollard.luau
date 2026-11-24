-- vnd:bollard on neoc-vm and neoc-vm2: what both answer the same without a Docker daemon. Connecting sends nothing,
-- so a client is made either way; what a call answers needs a daemon and is left to the module's own tests (a fake
-- daemon in the process, and the real one when asked for). What is refused is refused on both: neoc-vm returns the
-- refusal, neoc-vm2 raises a wrong argument (bollard.vm2.luau), and `refused` takes either.
local bollard = require("vnd:bollard") :: any
local out = {}

local function refused(call): boolean
	local ok, value, err = pcall(call)
	return not ok or (value == nil and err ~= nil)
end

-- a client is made without a daemon: port 1 is one nothing listens on
local docker, connect_err = bollard.connect({ host = "tcp://127.0.0.1:1", timeout = 5 })
out.connect = { type(docker), connect_err == nil }

-- the methods both have
local methods = {}
for _, name in {
	"ping",
	"version",
	"list_containers",
	"create_container",
	"start_container",
	"stop_container",
	"restart_container",
	"kill_container",
	"pause_container",
	"unpause_container",
	"remove_container",
	"logs",
	"exec",
	"list_images",
	"pull_image",
	"remove_image",
	"remove_volume",
	"create_network",
	"remove_network",
	"events",
} do
	table.insert(methods, type(docker[name]))
end
out.methods = methods

-- nothing answers: the first call says so
local pong, ping_err = docker:ping()
out.nothing_answers = { pong == nil, ping_err ~= nil }

-- refused before anything is sent
out.refused = {
	refused(function()
		return bollard.connect({ hots = "tcp://127.0.0.1:1" })
	end),
	refused(function()
		return bollard.connect({ host = "tcp://127.0.0.1:1", timeout = -1 })
	end),
	refused(function()
		return bollard.connect({ host = "gopher://127.0.0.1" })
	end),
	refused(function()
		return docker:start_container("../images/json")
	end),
	refused(function()
		return docker:stop_container("web", { signal = "TERM; rm" })
	end),
	refused(function()
		return docker:list_containers({ al = true })
	end),
	refused(function()
		return docker:remove_image("../containers/web")
	end),
	refused(function()
		return docker:exec("web", {})
	end),
}

return out
