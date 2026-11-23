-- vnd:hyper on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- A server on a loopback port the operating system picks, and `get` against it: nothing reaches the network and
-- nothing waits for a time limit. `post` and `request` take their arguments differently, and what an error is
-- differs, on purpose: see hyper.vm2.luau.
local net = require("std:net")
local hyper = require("vnd:hyper")
local out = {}

local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
local calls = 0
local server = assert(hyper.serve(listener, function(req)
	calls += 1
	local path = req.path
	if path == "/hello" then
		return { status = 200, headers = { ["content-type"] = "text/plain" }, body = "hello" }
	elseif path == "/text" then
		return "plain"
	elseif path == "/nothing" then
		return nil
	elseif path == "/empty" then
		return {}
	elseif path == "/request" then
		local headers = req.headers
		return {
			body = table.concat({
				req.method,
				req:header("X-Echo") or "none",
				headers["x-echo"] or "none",
				tostring(headers["X-Echo"]),
				tostring(req:header("x-missing")),
				tostring(#req.body),
				tostring(req.is_json),
			}, "|"),
		}
	elseif path == "/moved" then
		return { status = 302, headers = { location = "/hello" }, body = "moved" }
	elseif path == "/loop" then
		return { status = 307, headers = { location = "/loop" }, body = "again" }
	elseif path == "/cookies" then
		return { status = 204, headers = { ["x-served-by"] = "lua" } }
	elseif path == "/big" then
		return { body = string.rep("x", 2000) }
	elseif path == "/bytes" then
		return { body = "\xff\x00\xfe" }
	elseif path == "/raise" then
		error("no")
	end
	return { status = 404, body = "not found" }
end))
local addr = assert(server:addr())
local base = "http://" .. addr
out.addr = addr:match("^127%.0%.0%.1:%d+$") ~= nil

local function got(path: string, opts: { [string]: any }?): { any }
	local resp, err = hyper.get(base .. path, opts)
	if resp == nil then
		return { "failed", err ~= nil }
	end
	return { resp.status, resp.body, resp.url == base .. path }
end

local hello = assert(hyper.get(base .. "/hello"))
out.hello = { hello.status, hello.body, hello.headers["content-type"], hello.url == base .. "/hello" }
out.text = got("/text")
out.nothing = got("/nothing")
out.empty = got("/empty")
out.not_found = got("/anything-else")
out.query = got("/hello?x=1")
out.no_content = got("/cookies")
out.served_by = assert(hyper.get(base .. "/cookies")).headers["x-served-by"]
out.bytes = assert(hyper.get(base .. "/bytes")).body == "\xff\x00\xfe"

-- what the handler is given
out.request = got("/request")
out.request_with_headers = got("/request", { headers = { ["X-Echo"] = "ping" } })
out.host_replaced = got("/request", { headers = { Host = "virtual.example", ["x-echo"] = "pong" } })

-- a handler that raises answers 500, and the server serves on
out.raise = got("/raise")
out.after_raise = got("/hello")

-- redirects
local moved = assert(hyper.get(base .. "/moved"))
out.moved = { moved.status, moved.body, moved.url == base .. "/hello" }
local kept = assert(hyper.get(base .. "/moved", { follow_redirects = false }))
out.not_followed = { kept.status, kept.body, kept.headers.location, kept.url == base .. "/moved" }
out.followed_explicitly = got("/moved", { follow_redirects = true })[1]
out.loop = got("/loop")

-- the body cap
out.at_the_cap = #assert(hyper.get(base .. "/big", { max_body = 2000 })).body
out.over_the_cap = got("/big", { max_body = 1999 })

-- limits that are generous change nothing
out.generous = got("/hello", { timeout = 30, idle_timeout = 30, connect_timeout = 30, max_body = 2 ^ 40 })

-- a failure is `(nil, err)`; what `err` is differs
out.failures = {
	{ (function()
		local resp, err = hyper.get("not a url")
		return resp == nil and err ~= nil
	end)() },
	{ (function()
		local resp, err = hyper.get("ftp://example.com/")
		return resp == nil and err ~= nil
	end)() },
	{ (function()
		local resp, err = hyper.get("http://")
		return resp == nil and err ~= nil
	end)() },
	got("/hello", { timeout = 0 }),
	got("/hello", { idle_timeout = -1 }),
	got("/hello", { connect_timeout = math.huge }),
	got("/hello", { max_body = 0 }),
	got("/hello", { ca_pem = "nonsense" }),
	got("/hello", { headers = { ["bad name"] = "x" } }),
	got("/hello", { headers = { ["x-a"] = "line\r\nx-b: injected" } }),
	got("/hello", { headers = { ["Transfer-Encoding"] = "chunked" } }),
	got("/hello", { headers = { ["Content-Length"] = "3" } }),
	got("/hello", { headers = { Host = "a.example", host = "b.example" } }),
}

-- none of the refused requests reached the handler
local before = calls
got("/hello", { timeout = 0 })
got("/hello", { headers = { ["bad name"] = "x" } })
out.refused_before_sending = calls == before

-- a second server on a listener of its own, closed and waited for
local other = assert(hyper.serve(assert(net.TcpListener.bind("127.0.0.1:0")), function()
	return "other"
end))
out.other = assert(hyper.get("http://" .. assert(other:addr()) .. "/")).body
other:close()
other:wait()
other:wait()
out.waited = true

server:close()
server:wait()
return out
