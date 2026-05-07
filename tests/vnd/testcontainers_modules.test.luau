local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local hyper = require("vnd:hyper")
local testcontainers_modules = require("vnd:testcontainers_modules")


describe("testcontainers_modules.generic", function()

    test("starts an alpine container with cmd and stops it", function()
        local c, err = testcontainers_modules.generic({
            image = "alpine",
            tag   = "3.19",
            cmd   = { "sleep", "60" },
        })
        expect(err).toBeNil()
        expect(c).toBeTruthy()
        expect(c:host()).toBe("127.0.0.1")
        c:stop()
    end)

    test("exposes a port and resolves it via :port()", function()
        -- nginx publishes "start worker processes" on stderr once it's ready.
        local c = testcontainers_modules.generic({
            image = "nginx",
            tag   = "1.27-alpine",
            ports = { 80 },
            wait  = { type = "stderr", message = "start worker processes" },
        })
        local port = c:port(80)
        expect(type(port)).toBe("number")
        expect(port > 0).toBe(true)

        -- Smoke: actually hit the server end-to-end.
        local resp, herr = hyper.get("http://" .. c:host() .. ":" .. port .. "/")
        expect(herr).toBeNil()
        expect(resp.status).toBe(200)

        c:stop()
    end)

    test("missing required option returns error", function()
        local c, err = testcontainers_modules.generic({})
        expect(c).toBeNil()
        expect(err).toContain("testcontainers_modules.generic:")
    end)

    test("port not exposed at start raises a clear error", function()
        local c = testcontainers_modules.generic({
            image = "alpine",
            tag   = "3.19",
            cmd   = { "sleep", "60" },
        })
        local ok, err = pcall(function() return c:port(1234) end)
        expect(ok).toBe(false)
        expect(tostring(err)).toContain("not exposed")
        c:stop()
    end)

    test("env vars and unknown wait type error path", function()
        local c, err = testcontainers_modules.generic({
            image = "alpine",
            tag   = "3.19",
            cmd   = { "sleep", "60" },
            env   = { FOO = "bar" },
            wait  = { type = "nonsense" },
        })
        expect(c).toBeNil()
        expect(err).toContain("unknown wait type")
    end)

end)
