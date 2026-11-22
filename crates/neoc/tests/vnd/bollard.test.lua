local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local net = require("std:net")
local bollard = require("vnd:bollard")
local hyper = require("vnd:hyper")
local serde_json = require("vnd:serde_json")

-- A fake Docker daemon: a `vnd:hyper` server in this script that answers the Engine API's paths in Docker's
-- shapes, so the suite needs no Docker. (The Rust tests of the module cover the streams and every option.)
local seen = {}

local function json(status, value)
    return {
        status = status,
        headers = { ["content-type"] = "application/json" },
        body = (serde_json.to_string(value)),
    }
end

local function daemon(req)
    table.insert(seen, req.method .. " " .. req.path)
    if req.path == "/version" then
        return json(200, { Version = "25.0.3", ApiVersion = "1.44" })
    end
    if req.path == "/_ping" then
        return { status = 200, body = "OK" }
    end
    if req.method == "GET" and req.path == "/containers/json" then
        return json(200, {
            { Id = "aaa111", Names = { "/web" }, Image = "nginx:1.27", State = "running", Labels = { app = "web" } },
        })
    end
    if req.method == "GET" and req.path == "/containers/web/json" then
        return json(200, { Id = "aaa111", Name = "/web", Config = { Image = "nginx:1.27" } })
    end
    if req.method == "POST" and req.path == "/containers/create" then
        -- written out: an empty Luau table would be encoded as `{}`, and `Warnings` is a list
        return { status = 201, body = '{"Id":"ccc333","Warnings":[]}' }
    end
    if req.method == "POST" and req.path == "/containers/web/start" then
        return { status = 204 }
    end
    return json(404, { message = "No such container: ghost" })
end

local listener, listen_err = net.TcpListener.bind("127.0.0.1:0")
assert(listen_err == nil, "listener: " .. tostring(listen_err))
local server, serve_err = hyper.serve(listener, daemon)
assert(serve_err == nil, "serve: " .. tostring(serve_err))

local docker = assert(bollard.connect({ host = "tcp://" .. server:addr(), timeout = 10 }))

describe("bollard", function()

    describe("connect", function()
        test("a host with no socket is a connect error that names the path", function()
            local client, err = bollard.connect({ host = "unix:///nowhere/docker.sock" })
            expect(client).toBeNil()
            expect(err).toContain("bollard.connect: connect:")
            expect(err).toContain("/nowhere/docker.sock")
            expect(bollard.error_kind(err)).toBe("connect")
        end)

        test("an unknown option is refused", function()
            local client, err = bollard.connect({ hots = "tcp://127.0.0.1:1" })
            expect(client).toBeNil()
            expect(bollard.error_kind(err)).toBe("invalid_input")
        end)

        test("nothing answering is a connect error of the first call", function()
            -- connecting sends nothing, so it succeeds; port 1 is one nothing listens on
            local gone = assert(bollard.connect({ host = "tcp://127.0.0.1:1", timeout = 5 }))
            local ok, err = gone:ping()
            expect(ok).toBeNil()
            expect(err).toContain("bollard.ping: connect:")
        end)
    end)

    describe("containers", function()
        test("ping answers true", function()
            local ok, err = docker:ping()
            expect(err).toBeNil()
            expect(ok).toBe(true)
        end)

        test("a list is Docker's own tables", function()
            local list, err = docker:list_containers({ all = true })
            expect(err).toBeNil()
            expect(#list).toBe(1)
            expect(list[1].Names[1]).toBe("/web")
            expect(list[1].Labels.app).toBe("web")
        end)

        test("inspect returns the table, and nil for no such container", function()
            local web, err = docker:inspect_container("web")
            expect(err).toBeNil()
            expect(web.Config.Image).toBe("nginx:1.27")
            local ghost, ghost_err = docker:inspect_container("ghost")
            expect(ghost).toBeNil()
            expect(ghost_err).toBeNil()
        end)

        test("create returns the new id, start returns true", function()
            local made, err = docker:create_container({ name = "web", Image = "nginx:1.27" })
            expect(err).toBeNil()
            expect(made.Id).toBe("ccc333")
            expect(docker:start_container("web")).toBe(true)
        end)

        test("Docker's refusal keeps its message under a stable kind", function()
            local ok, err = docker:start_container("ghost")
            expect(ok).toBeNil()
            expect(err).toBe("bollard.start_container: not_found: No such container: ghost")
            expect(bollard.error_kind(err)).toBe("not_found")
        end)

        test("a name that is not one never reaches the daemon", function()
            local before = #seen
            local ok, err = docker:start_container("../images/json")
            expect(ok).toBeNil()
            expect(bollard.error_kind(err)).toBe("invalid_input")
            expect(#seen).toBe(before)
        end)
    end)

    describe("error_kind", function()
        test("is nil for a string that is not this module's, and for nil", function()
            expect(bollard.error_kind("hyper.get: connect: refused")).toBeNil()
            expect(bollard.error_kind(nil)).toBeNil()
        end)
    end)
end)

server:close()
