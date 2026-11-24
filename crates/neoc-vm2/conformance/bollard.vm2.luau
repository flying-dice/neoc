--!strict
-- intentional: vnd:bollard returns a typed DockerError (error_kind and error_message are gone); Docker's bodies and answers are bollard's models, typed field by field, so an unknown field is refused; create_container takes the body and then { name, platform }; names, signals and options that are refused are raised with their place; filter values are always lists; TLS is tls = { ca, cert, key }; logs, events and pull_image return the stream itself; info, inspect_container, stats, list_volumes, create_volume and list_networks are not ported yet
local bollard = require("vnd:bollard")

local function raised(call: () -> ()): string
	local ok, err = pcall(call :: () -> unknown)
	assert(not ok, "accepted")
	return tostring(err)
end

-- the functions that took an error text apart are gone: the error is a table
assert((bollard :: any).error_kind == nil and (bollard :: any).error_message == nil)

-- connecting sends nothing; what cannot be a client is a typed error
local docker = assert(bollard.connect({ host = "tcp://127.0.0.1:1", timeout = 5 }))
local none, scheme = bollard.connect({ host = "gopher://127.0.0.1" })
assert(none == nil and scheme ~= nil)
assert(scheme.kind == "invalid_input" and scheme.op == "vnd:bollard.connect")
assert(tostring(scheme) == "vnd:bollard.connect: invalid_input: " .. scheme.message)
local _, timeout = bollard.connect({ host = "tcp://127.0.0.1:1", timeout = 0 })
assert(timeout ~= nil and timeout.kind == "invalid_input")
if timeout.kind == "invalid_input" then
	assert(timeout.reason == 'option "timeout" must be more than zero seconds')
end

-- nothing listens on port 1: the first call's error, with its kind and Docker's (here the client's) words
local pong, unreachable = docker:ping()
assert(pong == nil and unreachable ~= nil)
assert(unreachable.kind == "connect" and unreachable.op == "vnd:bollard.Client:ping")
if unreachable.kind == "connect" then
	assert(unreachable.reason:find("the Docker daemon could not be reached", 1, true))
end
local list, list_err = docker:list_containers({ all = true, filters = { status = { "running" } } })
assert(list == nil and list_err ~= nil and list_err.kind == "connect")

-- a stream is returned as it is: the failure is its first next()'s, and then it is over
local logs = docker:logs("web", { tail = 10 })
local piece, logs_err = logs:next()
assert(piece == nil and logs_err ~= nil and logs_err.kind == "connect")
assert(logs_err.op == "vnd:bollard.Client:logs")
local after, after_err = logs:next()
assert(after == nil and after_err == nil)
local events = docker:events({ filters = { type = { "container" } } })
events:close()
local event, event_err = events:next()
assert(event == nil and event_err == nil)
assert(not pcall(function()
	for _ in docker:pull_image("nginx:1.27"):items() do
	end
end))

-- names are checked text: what is not one is refused with its place, before anything is sent
local place = "vnd:bollard.Client:start_container: argument #1 'name': the container must be a name or an id"
assert(raised(function()
	docker:start_container("../images/json")
end):find(place, 1, true))
assert(raised(function()
	docker:remove_volume("a/b")
end):find("argument #1 'name': the volume must be a name", 1, true))
assert(raised(function()
	docker:remove_network("")
end):find("argument #1 'name': the network must be a name", 1, true))
assert(raised(function()
	docker:pull_image("nginx?tag=x")
end):find("argument #1 'image': the image must be a reference", 1, true))
assert(raised(function()
	docker:kill_container("web", { signal = "HUP; rm" })
end):find("a signal is its name or its number", 1, true))

-- options are the call's own: an unknown one, a wrong type, a filter that is not a list
assert(raised(function()
	docker:list_containers({ al = true } :: any)
end):find("unknown field `al`", 1, true))
assert(raised(function()
	docker:list_containers({ filters = { status = "running" } } :: any)
end):find("filters", 1, true))
assert(raised(function()
	docker:logs("web", { tail = -1 })
end):find("argument #2 'opts'", 1, true))
assert(raised(function()
	bollard.connect({ tls = { ca = "ca.pem" } } :: any)
end):find("vnd:bollard.connect: argument #1 'opts'", 1, true))
assert(raised(function()
	bollard.connect({ tls_ca = "ca.pem" } :: any)
end):find("unknown field `tls_ca`", 1, true))

-- Docker's body is typed: a field it does not have, or of the wrong type, is refused with its path
assert(raised(function()
	docker:create_container({ Image = "nginx", name = "web" } :: any)
end):find("argument #1 'body': unknown field `name`", 1, true))
assert(raised(function()
	docker:create_container({ Image = "nginx", HostConfig = { Memroy = 1 } } :: any)
end):find("HostConfig: unknown field `Memroy`", 1, true))
assert(raised(function()
	docker:create_container({ Image = { "nginx" } } :: any)
end):find("Image: expected a string", 1, true))
assert(raised(function()
	docker:create_container({ Image = "nginx" }, { name = "a/b" })
end):find("the container must be a name or an id", 1, true))
assert(raised(function()
	docker:create_network({ Name = true } :: any)
end):find("Name: expected a string", 1, true))

-- an empty command is the call's own refusal
local ran, no_command = docker:exec("web", {})
assert(ran == nil and no_command ~= nil and no_command.kind == "invalid_input")

-- what a script writes type-checks; it is not run here, because it needs a daemon
local function _usage()
	local local_docker = assert(bollard.connect())
	local version = assert(local_docker:version())
	local _api: string? = version.ApiVersion
	-- Docker's map of port bindings has optional values, and luau-lsp does not infer a table literal against an
	-- optional map of optional values (`{ [string]: T? }?`): it is given its type in a local of its own
	local ports: { [string]: { vnd_bollard_PortBinding }? } = { ["80/tcp"] = { { HostPort = "8088" } } }
	local made, create_err = local_docker:create_container({
		Image = "nginx:1.27",
		Env = { "A=1" },
		Labels = { app = "web" },
		ExposedPorts = { ["80/tcp"] = {} },
		HostConfig = {
			Memory = 64 * 1024 * 1024,
			Binds = { "/srv/www:/usr/share/nginx/html:ro" },
			PortBindings = ports,
			RestartPolicy = { Name = "unless-stopped" },
		},
	}, { name = "web", platform = "linux/amd64" })
	if made == nil then
		assert(create_err ~= nil)
		if create_err.kind == "api" then
			local _status: number? = create_err.status
			local _reason: string = create_err.reason
		elseif create_err.kind == "too_large" then
			local _limit: number = create_err.limit
		elseif create_err.kind == "timed_out" then
			local _message: string = create_err.message
		end
		return
	end
	local _id: string = made.Id
	assert(local_docker:start_container(made.Id))
	for _, container in assert(local_docker:list_containers({ all = true, limit = 5, size = true })) do
		local _state: string? = container.State
		local names = container.Names
		if names ~= nil then
			local _first: string = names[1]
		end
	end
	local result = assert(local_docker:exec("web", { "nginx", "-t" }, { env = { "A=1" }, user = "root" }))
	local _code: number? = result.exit_code
	local _out: string = result.stdout .. result.stderr
	for log in local_docker:logs("web", { follow = true, since = 0, timestamps = true }):items() do
		local _from: "stdout" | "stderr" | "stdin" | "console" = log.stream
		local _data: string = log.data
	end
	local pull = local_docker:pull_image("nginx", { platform = "linux/amd64" })
	local progress, pull_err = pull:next()
	if progress ~= nil then
		local _status: string? = progress.status
		local detail = progress.progressDetail
		if detail ~= nil then
			local _total: number? = detail.total
		end
	elseif pull_err ~= nil then
		local _kind: string = pull_err.kind
	end
	pull:close()
	local happened = local_docker:events({ since = 0, filters = { event = { "start", "die" } } }):next()
	if happened ~= nil then
		local _action: string? = happened.Action
		local actor = happened.Actor
		if actor ~= nil then
			local _actor: string? = actor.ID
		end
	end
	for _, image in assert(local_docker:list_images({ digests = true })) do
		local _tags: { string }? = image.RepoTags
		local _size: number = image.Size
	end
	for _, removed in assert(local_docker:remove_image("nginx:1.27", { force = true, noprune = true })) do
		local _untagged: string? = removed.Untagged
	end
	local network = assert(local_docker:create_network({ Name = "backend", Driver = "bridge", Labels = { app = "web" } }))
	local _network: string = network.Id
	assert(local_docker:remove_network("backend"))
	assert(local_docker:stop_container("web", { timeout = 5, signal = "SIGINT" }))
	assert(local_docker:restart_container("web"))
	assert(local_docker:kill_container("web", { signal = "SIGHUP" }))
	assert(local_docker:pause_container("web"))
	assert(local_docker:unpause_container("web"))
	assert(local_docker:remove_container("web", { force = true, volumes = true }))
	assert(local_docker:remove_volume("data", { force = true }))
end

-- the module cannot be patched
assert(not pcall(function()
	(bollard :: any).connect = nil
end))

return true
