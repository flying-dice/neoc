-- Exercises hyper.serve(listener, handler) with Lua-side dispatch:
-- path params, per-method routing and 404 fallback all live in the
-- handler now (the service_fn model). Multi-worker fan-out
-- (runtime.shared on the listener) is covered by tests/multi_worker_subprocess.rs.

local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local TcpListener = require("std:net").TcpListener
local hyper = require("vnd:hyper")


-- A counter the handler closes over — proves Lua closures work as the
-- per-worker-shared-state mechanism. Here we only have one engine, but
-- the closure-capture path is identical to multi-worker mode.
local hits = 0

local listener = TcpListener.bind("127.0.0.1:0")

local function service(req)
    if req.method == "GET" and req.path == "/" then
        hits = hits + 1
        return "OK"
    end

    local id = req.path:match("^/users/([^/]+)$")
    if req.method == "GET" and id then
        return { status = 200, body = "id=" .. id }
    end

    local dir, name = req.path:match("^/files/([^/]+)/([^/]+)$")
    if req.method == "GET" and dir then
        return { status = 200, body = dir .. "/" .. name }
    end

    if req.method == "POST" and req.path == "/echo" then
        return { status = 200, body = req.body }
    end

    if req.path == "/items/1" then
        if req.method == "PUT"    then return { status = 204 } end
        if req.method == "PATCH"  then return { status = 200, body = "patched" } end
        if req.method == "DELETE" then return { status = 204 } end
    end

    return { status = 404 }
end

local app = hyper.serve(listener, service)
local BASE = "http://" .. app:addr()

describe("hyper.serve", function()

    describe("addr", function()
        test("returns the bound address", function()
            expect(app:addr()).toContain("127.0.0.1:")
        end)
    end)

    describe("get", function()
        test("string-return is treated as a 200 body", function()
            local resp, err = hyper.get(BASE .. "/")
            expect(err).toBeNil()
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("OK")
        end)

        test("handler closures capture upvalues", function()
            local before = hits
            hyper.get(BASE .. "/")
            hyper.get(BASE .. "/")
            expect(hits).toBe(before + 2)
        end)
    end)

    describe("path params (Lua-side)", function()
        test("single capture", function()
            local resp = hyper.get(BASE .. "/users/42")
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("id=42")
        end)

        test("multiple captures", function()
            local resp = hyper.get(BASE .. "/files/docs/readme.md")
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("docs/readme.md")
        end)

        test("wrong segment count falls through to 404", function()
            local resp = hyper.get(BASE .. "/users")
            expect(resp.status).toBe(404)
        end)
    end)

    describe("method routing (Lua-side)", function()
        test("post handler receives body", function()
            local resp = hyper.post({ url = BASE .. "/echo", body = "ping" })
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("ping")
        end)

        test("put", function()
            local resp = hyper.request({ method = "PUT", url = BASE .. "/items/1" })
            expect(resp.status).toBe(204)
        end)

        test("patch", function()
            local resp = hyper.request({ method = "PATCH", url = BASE .. "/items/1" })
            expect(resp.status).toBe(200)
            expect(resp.body).toBe("patched")
        end)

        test("delete", function()
            local resp = hyper.request({ method = "DELETE", url = BASE .. "/items/1" })
            expect(resp.status).toBe(204)
        end)

        test("wrong method falls through to 404", function()
            local resp = hyper.request({ method = "POST", url = BASE .. "/" })
            expect(resp.status).toBe(404)
        end)
    end)

    describe("close", function()
        test("close on a still-listening server is a no-op error path", function()
            local l = TcpListener.bind("127.0.0.1:0")
            local s = hyper.serve(l, function() return "x" end)
            s:close()
            -- close() doesn't return anything; just assert it didn't throw.
            expect(true).toBe(true)
        end)
    end)
end)

app:close()
