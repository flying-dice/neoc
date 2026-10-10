local _t      = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local http = require("lib:http")

describe("http", function()

    describe("module surface", function()
        test("serve_static is a function", function()
            expect(type(http.serve_static)).toBe("function")
        end)
    end)

end)
