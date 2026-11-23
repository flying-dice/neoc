-- lib:test on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- (It prints its suites and tests, as it does for a test script.) A matcher's assertions are closures on neoc-vm
-- (`expect(x).toBe(y)`) and methods on neoc-vm2 (`expect(x):toBe(y)`), so no assertion is called here: see
-- test.vm2.luau.
local test = require("lib:test")

local out = {}

-- bodies run in order, at once; a failed test raises nothing and the ones after it run
local order: { string } = {}
local returned = {
	select("#", test.test("alone", function()
		table.insert(order, "alone")
	end)),
	select("#", test.describe("suite", function()
		table.insert(order, "suite")
		test.test("first", function()
			table.insert(order, "first")
		end)
		test.test("raises", function()
			table.insert(order, "raises")
			error("boom")
		end)
		test.describe("inner", function()
			test.test("second", function()
				table.insert(order, "second")
			end)
		end)
		test.test("third", function()
			table.insert(order, "third")
		end)
	end)),
}
out.order = order
out.returned = returned

-- what a suite's body raises outside a test is raised by describe; a suite after it runs
local ok = pcall(test.describe, "broken", function()
	error("outside a test")
end)
local after = false
test.describe("after", function()
	after = true
end)
out.broken_suite = { ok, after }

-- expect takes anything, and gives something to assert on
out.expect = { test.expect(1) ~= nil, test.expect(nil) ~= nil, test.expect({}) ~= nil, test.expect(print) ~= nil }

-- a body is a function
out.wrong_arguments = { (pcall(test.test, "name")), (pcall(test.test, "name", "body")), (pcall(test.describe, "name", 5)) }

return out
