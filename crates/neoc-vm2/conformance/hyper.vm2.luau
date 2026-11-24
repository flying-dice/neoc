--!strict
-- intentional: vnd:hyper's errors are typed (HyperError: err.kind, limit, max_redirects, max_body, reason; error_kind and error_message are gone); post(url, body?, opts?) and request(method, url, body?, opts?) take the URL, method and body as arguments; follow_redirects is a boolean and max_redirects the limit; a misspelt or wrongly typed option, a buffer or number body and a number header value are refused as wrong arguments; a handler returns a string, a Reply or nothing; serve returns a typed ServerError and Server:addr() returns (nil, err) once closed
local net = require("std:net")
local hyper = require("vnd:hyper")

assert((hyper :: any).error_kind == nil and (hyper :: any).error_message == nil)

local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
local server = assert(hyper.serve(listener, function(req)
	local path = req.path
	if path == "/echo" then
		return { body = req.method .. ":" .. req.body .. ":" .. (req:header("content-length") or "none") }
	elseif path == "/json" then
		local json = req.json
		return { body = tostring(req.is_json) .. ":" .. type(json) }
	elseif path == "/moved" then
		return { status = 302, headers = { location = "/echo" } }
	elseif path == "/loop" then
		return { status = 307, headers = { location = "/loop" } }
	elseif path == "/big" then
		return string.rep("x", 100)
	elseif path == "/nothing" then
		return nil
	elseif path == "/typo" then
		-- a key a Reply does not have: the request fails, the server serves on
		return { status = 200, bodyy = "x" } :: any
	elseif path == "/number" then
		return 42 :: any
	end
	return { status = 404 }
end))
local addr, addr_err = server:addr()
assert(addr ~= nil and addr_err == nil)
local base = "http://" .. addr

-- a response is a table of these four
local resp, err = hyper.get(base .. "/echo")
assert(resp ~= nil and err == nil)
assert(resp.status == 200 and resp.body == "GET::none" and resp.url == base .. "/echo")
assert(type(resp.headers) == "table" and resp.headers["content-length"] == "9")

-- the URL, the method and the body are arguments
local posted = assert(hyper.post(base .. "/echo", "ping"))
assert(posted.body == "POST:ping:4")
assert(assert(hyper.post(base .. "/echo")).body == "POST::0")
assert(assert(hyper.post(base .. "/echo", nil, { timeout = 30 })).body == "POST::0")
assert(assert(hyper.post(base .. "/echo", "\xff\x00")).body == "POST:\xff\x00:2")
assert(assert(hyper.request("put", base .. "/echo", "abc")).body == "PUT:abc:3")
assert(assert(hyper.request("DELETE", base .. "/echo")).body == "DELETE::none")
assert(assert(hyper.request("PuRgE", base .. "/echo", nil, { headers = { accept = "*/*" } })).body == "PuRgE::none")

-- the body a handler reads as JSON is lib:json's typed node
local as_json = assert(hyper.post(base .. "/json", '{"a":1}', { headers = { ["content-type"] = "application/json" } }))
assert(as_json.body == "true:userdata")
assert(assert(hyper.post(base .. "/json", "7")).body == "false:number")
assert(assert(hyper.post(base .. "/json", "not json")).body == "false:nil")

-- a handler answers with a string, a Reply or nothing; anything else is a 500
assert(#assert(hyper.get(base .. "/big")).body == 100)
local nothing = assert(hyper.get(base .. "/nothing"))
assert(nothing.status == 200 and nothing.body == "")
assert(assert(hyper.get(base .. "/missing")).status == 404)
for _, path in { "/typo", "/number" } do
	local failed = assert(hyper.get(base .. path))
	assert(failed.status == 500 and failed.body == "handler error", path)
end

-- a failure is a typed error: its kind, what the kind carries, the call, and a message
local _, invalid = hyper.get("ftp://example.com/")
assert(invalid ~= nil and invalid.kind == "invalid_url" and invalid.op == "vnd:hyper.get")
assert(invalid.message == invalid.reason)
assert(tostring(invalid) == "vnd:hyper.get: invalid_url: " .. invalid.message)

local _, too_many = hyper.get(base .. "/loop", { max_redirects = 3 })
assert(too_many ~= nil and too_many.kind == "too_many_redirects" and too_many.max_redirects == 3)
assert(too_many.message == "more than 3 redirects")
local _, by_default = hyper.get(base .. "/loop")
assert(by_default ~= nil and by_default.kind == "too_many_redirects" and by_default.max_redirects == 16)

local _, too_large = hyper.request("GET", base .. "/big", nil, { max_body = 99 })
assert(too_large ~= nil and too_large.kind == "too_large" and too_large.max_body == 99)
assert(too_large.op == "vnd:hyper.request")
assert(tostring(too_large) == "vnd:hyper.request: too_large: the response body is longer than max_body (99 bytes)")

-- follow_redirects is a boolean; false follows none whatever the limit
local kept = assert(hyper.get(base .. "/moved", { follow_redirects = false, max_redirects = 5 }))
assert(kept.status == 302 and kept.headers.location == "/echo")
assert(assert(hyper.get(base .. "/moved", { max_redirects = 1 })).status == 200)

-- what an option's type cannot say is `invalid_input`, before anything is sent
local function invalid_input(what: string, value: unknown, why: vnd_hyper_HyperError?)
	assert(value == nil and why ~= nil and why.kind == "invalid_input", what)
	assert(why.reason:find(what, 1, true), why.reason)
end
invalid_input("timeout must be", hyper.get(base .. "/echo", { timeout = 0 }))
invalid_input("idle_timeout must be", hyper.get(base .. "/echo", { idle_timeout = -1 }))
invalid_input("connect_timeout must be", hyper.get(base .. "/echo", { connect_timeout = math.huge }))
invalid_input("max_body must be", hyper.get(base .. "/echo", { max_body = 0 }))
invalid_input("max_redirects must be", hyper.get(base .. "/echo", { max_redirects = 0 }))
invalid_input("ca_pem holds no certificate", hyper.get(base .. "/echo", { ca_pem = "nonsense" }))
invalid_input("is not a valid header name", hyper.get(base .. "/echo", { headers = { ["bad name"] = "x" } }))
invalid_input("Content-Length", hyper.post(base .. "/echo", "abc", { headers = { ["Content-Length"] = "1" } }))
invalid_input("method is not a valid HTTP method name", hyper.request("GE T", base .. "/echo"))

-- what is not an option, or not of its type, is refused as a wrong argument with its place
local function refused(what: string, f: (...any) -> ...any, ...: any)
	local ok, why = pcall(f, ...)
	assert(not ok and tostring(why):find(what, 1, true), what .. ": " .. tostring(why))
end
refused("unknown field `timeuot`", hyper.get, base, { timeuot = 5 })
refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base, { timeout = "5" })
refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base, { follow_redirects = 3 })
refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base, { headers = { ["x-n"] = 5 } })
refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base, { max_body = 1.5 })
refused("vnd:hyper.get: argument #1 'url'", hyper.get)
refused("vnd:hyper.post: argument #1 'url'", hyper.post, { url = base, body = "x" })
refused("vnd:hyper.post: argument #2 'body'", hyper.post, base, 42)
refused("vnd:hyper.post: argument #2 'body'", hyper.post, base, buffer.fromstring("bytes"))
refused("vnd:hyper.request: argument #1 'method'", hyper.request, { method = "GET", url = base })
refused("vnd:hyper.serve: argument #1 'listener'", hyper.serve, "127.0.0.1:0", function() end)
refused("vnd:hyper.serve: argument #2 'handler'", hyper.serve, listener, "handler")

-- a closed server has no address to give
server:close()
server:wait()
local gone, closed = server:addr()
assert(gone == nil and closed ~= nil and closed.kind == "closed" and closed.op == "vnd:hyper.Server:addr")
assert(tostring(closed) == "vnd:hyper.Server:addr: closed: the server is closed")
