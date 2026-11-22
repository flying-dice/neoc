local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local net = require("std:net")
local hyper = require("vnd:hyper")

local TcpListener = net.TcpListener

-- Counter the proxy handler closes over — proves upvalue capture works.
local handler_calls = 0

-- Forward declared so the proxy handler can reference the server's own URL.
local server_base

local listener, lerr = TcpListener.bind("127.0.0.1:0")
assert(lerr == nil, "listener: " .. tostring(lerr))

-- Single service_fn-style handler: dispatches on req.method / req.path.
local function service(req)
    if req.method == "GET" and req.path == "/hello" then
        return {
            status = 200,
            headers = { ["content-type"] = "text/plain" },
            body = "hello",
        }
    end
    if req.method == "GET" and req.path == "/headers" then
        return { status = 200, body = req:header("x-echo") or "" }
    end
    if req.path == "/length" then
        return { status = 200, body = req:header("content-length") or "none" }
    end
    if req.method == "POST" and req.path == "/echo" then
        return { status = 200, body = req.body }
    end
    if req.method == "PUT" and req.path == "/items/1" then
        return { status = 204 }
    end
    if req.method == "GET" and req.path == "/handler/greet" then
        handler_calls = handler_calls + 1
        return {
            status  = 200,
            headers = { ["x-handled-by"] = "lua" },
            body    = "hi from lua handler, " .. req.method,
        }
    end
    if req.method == "POST" and req.path == "/handler/echo" then
        handler_calls = handler_calls + 1
        return { status = 200, body = req.body }
    end
    if req.method == "GET" and req.path == "/handler/proxy" then
        handler_calls = handler_calls + 1
        -- Killer demo: handler re-enters the server via the async client.
        -- Proves: (1) async lib calls work inside a handler, and (2) the
        -- handler captures `server_base` from the enclosing scope.
        local inner, ierr = hyper.get(server_base .. "/hello")
        if ierr ~= nil then
            return { status = 502, body = "proxy err: " .. ierr }
        end
        return { status = inner.status, body = "proxied:" .. inner.body }
    end
    return { status = 404, body = "not found" }
end

local server, serve_err = hyper.serve(listener, service)
assert(serve_err == nil, "serve: " .. tostring(serve_err))

server_base = "http://" .. server:addr()
local BASE = server_base

describe("hyper", function()

    describe("serve", function()
        test("addr() returns the bound address", function()
            expect(server:addr()).toContain("127.0.0.1:")
        end)

        test("listener on bad address returns error", function()
            local listener_ud, le = TcpListener.bind("not an addr")
            expect(listener_ud).toBeNil()
            expect(le).toContain("TcpListener.bind:")
        end)
    end)

    -- get ---------------------------------------------------------------------

    describe("get", function()
        test("returns 200 and body", function()
            local resp, err = hyper.get(BASE .. "/hello")
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("hello")
        end)

        test("response headers are returned", function()
            local resp, err = hyper.get(BASE .. "/hello")
            expect(err).toBeNil()
            expect(resp.headers["content-type"]).toBe("text/plain")
        end)

        test("invalid url returns error", function()
            local resp, err = hyper.get("not a url")
            expect(resp).toBeNil()
            expect(err).toContain("hyper.get:")
        end)

        test("a scheme other than http and https is rejected", function()
            local resp, err = hyper.get("ftp://example.com/")
            expect(resp).toBeNil()
            expect(err).toContain("hyper.get:")
            expect(err).toContain("only http:// and https://")
        end)

        -- needs network access: the certificate is checked against the platform trust store
        test("https request to a public host succeeds", function()
            local resp, err = hyper.get("https://example.com/")
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toContain("Example Domain")
        end)

        test("a ca_pem that is not PEM is an error", function()
            local resp, err = hyper.get("https://example.com/", { ca_pem = "nonsense" })
            expect(resp).toBeNil()
            expect(err).toContain("ca_pem")
        end)

        test("headers in opts are sent", function()
            local resp, err = hyper.get(BASE .. "/headers", { headers = { ["x-echo"] = "from get" } })
            expect(err).toBeNil()
            expect(resp.body).toBe("from get")
        end)

        test("headers that are not a table are an error", function()
            local resp, err = hyper.get(BASE .. "/headers", { headers = "x-echo: ping" })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.get: headers must be a table, got string")
        end)

        test("an unknown option is an error that names it, not dropped", function()
            local resp, err = hyper.get(BASE .. "/headers", { header = { ["x-echo"] = "typo" } })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.get: unknown option `header` (expected one of: headers, ca_pem)")
        end)

        test("a GET sends no content-length", function()
            local resp, err = hyper.get(BASE .. "/length")
            expect(err).toBeNil()
            expect(resp.body).toBe("none")
        end)

        test("connection failure returns error", function()
            local resp, err = hyper.get("http://127.0.0.1:1/")
            expect(resp).toBeNil()
            expect(err).toContain("hyper.get:")
        end)
    end)

    -- post --------------------------------------------------------------------

    describe("post", function()
        test("body is echoed", function()
            local resp, err = hyper.post({ url = BASE .. "/echo", body = "ping pong" })
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("ping pong")
        end)

        test("empty body succeeds", function()
            local resp, err = hyper.post({ url = BASE .. "/echo", body = "" })
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("")
        end)

        test("missing url returns error", function()
            local resp, err = hyper.post({})
            expect(resp).toBeNil()
            expect(err).toContain("hyper.post:")
        end)

        test("a buffer body is sent as its bytes", function()
            local body = buffer.create(4)
            buffer.writeu32(body, 0, 0x00FF0141)
            local resp, err = hyper.post({ url = BASE .. "/echo", body = body })
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("\x41\x01\xFF\x00")
        end)

        test("a body that is neither a string nor a buffer is an error, not an empty body", function()
            local resp, err = hyper.post({ url = BASE .. "/echo", body = { "ping" } })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.post: body must be a string or a buffer, got table")
            resp, err = hyper.post({ url = BASE .. "/echo", body = true })
            expect(resp).toBeNil()
            expect(err).toContain("got boolean")
        end)

        test("an unknown option is an error that names it", function()
            local resp, err = hyper.post({ url = BASE .. "/echo", body = "x", timeout = 5 })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.post: unknown option `timeout` (expected one of: url, body, headers, ca_pem)")
        end)

        test("no body or an empty one is sent with content-length 0", function()
            for _, body in ipairs({ "", buffer.create(0) }) do
                local resp, err = hyper.post({ url = BASE .. "/length", body = body })
                expect(err).toBeNil()
                expect(resp.body).toBe("0")
            end
            local resp, err = hyper.post({ url = BASE .. "/length" })
            expect(err).toBeNil()
            expect(resp.body).toBe("0")
            resp, err = hyper.post({ url = BASE .. "/length", body = "abcd" })
            expect(err).toBeNil()
            expect(resp.body).toBe("4")
        end)

        test("a number body is sent as its text", function()
            local resp, err = hyper.post({ url = BASE .. "/echo", body = 42 })
            expect(err).toBeNil()
            expect(resp.body).toBe("42")
        end)
    end)

    -- request -----------------------------------------------------------------

    describe("request", function()
        test("custom request headers are forwarded", function()
            local resp, err = hyper.request({
                method  = "GET",
                url     = BASE .. "/headers",
                headers = { ["x-echo"] = "ping" },
            })
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("ping")
        end)

        test("PUT with custom method works", function()
            local resp, err = hyper.request({
                method = "PUT",
                url    = BASE .. "/items/1",
            })
            expect(err).toBeNil()
            expect(resp.status).toBe(204)
        end)

        test("missing method returns error", function()
            local resp, err = hyper.request({ url = BASE .. "/hello" })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.request:")
        end)

        test("missing url returns error", function()
            local resp, err = hyper.request({ method = "GET" })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.request:")
        end)

        test("a scheme other than http and https is rejected", function()
            local resp, err = hyper.request({
                method = "GET",
                url    = "ftp://example.com/",
            })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.request:")
            expect(err).toContain("only http:// and https://")
        end)

        test("headers that are not a table are an error, not no headers", function()
            local resp, err = hyper.request({ method = "GET", url = BASE .. "/headers", headers = "x-echo: ping" })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.request: headers must be a table, got string")
        end)

        test("an unknown option is an error that names it", function()
            local resp, err = hyper.request({ method = "GET", url = BASE .. "/headers", hedaers = {} })
            expect(resp).toBeNil()
            expect(err).toContain("hyper.request: unknown option `hedaers`")
        end)

        test("a PUT with no body says content-length 0, a DELETE with none sends no length", function()
            local resp, err = hyper.request({ method = "PUT", url = BASE .. "/length" })
            expect(err).toBeNil()
            expect(resp.body).toBe("0")
            resp, err = hyper.request({ method = "DELETE", url = BASE .. "/length" })
            expect(err).toBeNil()
            expect(resp.body).toBe("none")
        end)

        test("a buffer body goes through request too", function()
            local resp, err = hyper.request({ method = "POST", url = BASE .. "/echo", body = buffer.fromstring("put") })
            expect(err).toBeNil()
            expect(resp.body).toBe("put")
        end)

        test("a ca_pem that is not a string is an error", function()
            local resp, err = hyper.request({
                method = "GET",
                url    = "https://example.com/",
                ca_pem = {},
            })
            expect(resp).toBeNil()
            expect(err).toContain("ca_pem must be a string")
        end)
    end)

    -- handler -----------------------------------------------------------------

    describe("handler", function()
        test("handler responds with custom status and body", function()
            local resp, err = hyper.get(BASE .. "/handler/greet")
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("hi from lua handler, GET")
        end)

        test("handler response headers are forwarded", function()
            local resp, err = hyper.get(BASE .. "/handler/greet")
            expect(err).toBeNil()
            expect(resp.headers["x-handled-by"]).toBe("lua")
        end)

        test("handler sees request body", function()
            local resp, err = hyper.post({
                url  = BASE .. "/handler/echo",
                body = "from-test",
            })
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("from-test")
        end)

        test("unmatched path returns 404", function()
            local resp, err = hyper.get(BASE .. "/anything-unhandled")
            expect(err).toBeNil()
            expect(resp.status).toBe(404)
        end)

        test("handler can call hyper.get internally (upvalue capture)", function()
            local before = handler_calls
            local resp, err = hyper.get(BASE .. "/handler/proxy")
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("proxied:hello")
            -- Outer request lands on /handler/proxy; the inner /hello call
            -- is dispatched by the same service function.
            expect(handler_calls - before).toBe(1)
        end)
    end)

    -- close -------------------------------------------------------------------

    describe("close", function()
        test("close on a separate server is callable", function()
            local s = hyper.serve(
                TcpListener.bind("127.0.0.1:0"),
                function() return "x" end
            )
            local r, e = hyper.get("http://" .. s:addr() .. "/x")
            expect(e).toBeNil()
            expect(r.status).toBe(200)
            s:close()
        end)
    end)

    -- wait --------------------------------------------------------------------

    describe("wait", function()
        -- Contract: wait() must return cleanly once close() has been called.
        -- Regression (e.g. wait deadlocking) would manifest as a runner hang.
        test("returns after close is called", function()
            local s = hyper.serve(
                TcpListener.bind("127.0.0.1:0"),
                function() return "x" end
            )
            s:close()
            s:wait()
        end)

        -- A second wait() after the server is already done must also return
        -- (watch channel preserves the latest value, so late callers see it).
        test("is safe to call after the server has stopped", function()
            local s = hyper.serve(
                TcpListener.bind("127.0.0.1:0"),
                function() return "x" end
            )
            s:close()
            s:wait()
            s:wait()
        end)
    end)
end)

server:close()
