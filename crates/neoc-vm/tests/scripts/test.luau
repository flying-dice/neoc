--!strict
-- lib:test as a script sees it: its answers (test.json), then what it asserts. (It prints its suites and tests, as it
-- does for a test script.)
local test = require("lib:test")

local out = {}

-- bodies run in order, at once; a failed test raises nothing and the ones after it run
local order: { string } = {}
local returned = {
  select(
    "#",
    test.test("alone", function()
      table.insert(order, "alone")
    end)
  ),
  select(
    "#",
    test.describe("suite", function()
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
    end)
  ),
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
out.expect = {
  (test.expect(1) :: any) ~= nil,
  (test.expect(nil) :: any) ~= nil,
  (test.expect {} :: any) ~= nil,
  (test.expect(print) :: any) ~= nil,
}

-- a body is a function
out.wrong_arguments = {
  (pcall(test.test :: any, "name")),
  (pcall(test.test :: any, "name", "body")),
  (pcall(test.describe :: any, "name", 5)),
}

-- what it asserts
do
  local expect = test.expect

  -- what holds raises nothing and returns nothing
  local shared = {}
  assert(select("#", expect(1):toBe(1)) == 0)
  expect("a"):toBe("a")
  expect(nil):toBe(nil)
  expect(shared):toBe(shared)
  expect(nil):toBeNil()
  expect(0):toBeTruthy()
  expect(""):toBeTruthy()
  expect(nil):toBeFalsy()
  expect(false):toBeFalsy()
  expect("hello world"):toContain("lo w")
  expect("abc"):toContain("")
  expect({ "a", "b" }):toContain("b")
  expect({ key = shared }):toContain(shared)
  expect(function()
    error("no")
  end):toThrow()
  expect(function()
    error { code = 1 }
  end):toThrow()

  -- what does not hold raises an error that says which matcher, why, and with what
  local function raised(assertion: () -> ...unknown): string
    local ok, err = pcall(assertion)
    assert(not ok, "the expectation held")
    return tostring(err)
  end
  local function starts(text: string, with: string)
    assert(text:sub(1, #with) == with, text)
  end
  starts(
    raised(function()
      expect(1):toBe(2)
    end),
    "lib:test.Matcher:toBe: not_equal: expected 1 to be 2"
  )
  starts(
    raised(function()
      expect("1"):toBe(1)
    end),
    'lib:test.Matcher:toBe: not_equal: expected "1" to be 1'
  )
  starts(
    raised(function()
      expect({}):toBe {}
    end),
    "lib:test.Matcher:toBe: not_equal: expected <table> to be <table>"
  )
  starts(
    raised(function()
      expect(false):toBeNil()
    end),
    "lib:test.Matcher:toBeNil: not_nil: expected nil, got false"
  )
  starts(
    raised(function()
      expect(nil):toBeTruthy()
    end),
    "lib:test.Matcher:toBeTruthy: not_truthy: expected truthy, got nil"
  )
  starts(
    raised(function()
      expect(0):toBeFalsy()
    end),
    "lib:test.Matcher:toBeFalsy: not_falsy: expected falsy, got 0"
  )
  starts(
    raised(function()
      expect("abc"):toContain("d")
    end),
    'lib:test.Matcher:toContain: not_contained: "abc" does not contain "d"'
  )
  starts(
    raised(function()
      expect({ "a" }):toContain("b")
    end),
    'lib:test.Matcher:toContain: not_contained: table does not contain "b"'
  )
  starts(
    raised(function()
      expect(5):toContain(5)
    end),
    "lib:test.Matcher:toContain: not_a_container: toContain requires a string or table value"
  )
  starts(
    raised(function()
      expect(function() end):toThrow()
    end),
    "lib:test.Matcher:toThrow: did_not_throw: expected function to throw, but it returned normally"
  )
  starts(
    raised(function()
      expect(5):toThrow()
    end),
    "lib:test.Matcher:toThrow: not_a_function: toThrow requires a function value"
  )

  -- a failed expectation fails its test and no other; the tests after it run
  local ran: { string } = {}
  test.describe("outer", function()
    test.test("fails", function()
      table.insert(ran, "fails")
      expect(1):toBe(2)
      table.insert(ran, "not reached")
    end)
    test.describe("inner", function()
      test.test("passes", function()
        table.insert(ran, "inner")
        expect(#ran):toBe(2)
      end)
    end)
    test.test("still in the outer suite", function()
      table.insert(ran, "outer")
    end)
  end)
  assert(table.concat(ran, ",") == "fails,inner,outer")

  -- there is one name for a test
  assert((test :: any).it == nil)

  -- arguments of the wrong shape are refused, with their place
  local function refused(place: string, call: () -> ...unknown)
    starts(raised(call):gsub("^runtime error: ", ""), place)
  end
  refused("lib:test.test: argument #2 'body': expected a function, got nil", function()
    return (test.test :: any)("name")
  end)
  refused("lib:test.describe: argument #1 'name': expected a string, got function", function()
    return (test.describe :: any)(function() end)
  end)
  refused("lib:test.expect: takes 1 argument, got more", function()
    return (test.expect :: any)(1, 2)
  end)
  refused("lib:test.Matcher:toBeNil: takes 0 arguments, got more", function()
    return (expect(nil) :: any):toBeNil(nil)
  end)
end

return out
