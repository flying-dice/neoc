-- `table.sort` for scripts (installed by `app_env::new_env`, replacing the C one).
--
-- Why: the C sort is one VM instruction, so the CPU budget cannot see inside it. With no comparison function it
-- runs without a single tick (sorting a million numbers is free), and quicksort has inputs that take quadratic
-- time (a few hundred thousand elements run for half a minute, well past every wall-clock limit). Written in
-- Lua, every step of the sort is counted by the VM's count hook like any other loop of the script, the running
-- time is O(n log n) whatever the input, and a comparison function may call the host (an async host call can
-- yield here; inside the C sort it could not).
--
-- It is a stable merge sort over a copy: insertion sort on runs of RUN elements, then bottom-up merges. Unlike
-- the C sort it never raises "invalid order function for sorting": an inconsistent comparison function gives
-- some order of the same elements. `t` is read and written with plain indexing, in 1..#t, as the C version.
-- The comparison errors are the same ("attempt to compare ..."), and so are the argument errors.

local type, error, tostring = type, error, tostring

local RUN = 8

-- Sorts the first n elements of `a` by `<`, using `b` as scratch; returns the array holding the result.
local function sort_default(a, b, n)
  for lo = 1, n, RUN do
    local hi = lo + RUN - 1
    if hi > n then
      hi = n
    end
    for i = lo + 1, hi do
      local v = a[i]
      local j = i - 1
      while j >= lo and v < a[j] do
        a[j + 1] = a[j]
        j = j - 1
      end
      a[j + 1] = v
    end
  end
  local width = RUN
  while width < n do
    local k = 1
    for lo = 1, n, 2 * width do
      local mid = lo + width
      local hi = lo + 2 * width
      if mid > n + 1 then
        mid = n + 1
      end
      if hi > n + 1 then
        hi = n + 1
      end
      local i, j = lo, mid
      while i < mid and j < hi do
        local x, y = a[i], a[j]
        if y < x then
          b[k] = y
          j = j + 1
        else
          b[k] = x
          i = i + 1
        end
        k = k + 1
      end
      while i < mid do
        b[k] = a[i]
        i = i + 1
        k = k + 1
      end
      while j < hi do
        b[k] = a[j]
        j = j + 1
        k = k + 1
      end
    end
    a, b = b, a
    width = width * 2
  end
  return a
end

-- The same with a comparison function: `less(y, x)` is true when y must come before x.
local function sort_with(less, a, b, n)
  for lo = 1, n, RUN do
    local hi = lo + RUN - 1
    if hi > n then
      hi = n
    end
    for i = lo + 1, hi do
      local v = a[i]
      local j = i - 1
      while j >= lo and less(v, a[j]) do
        a[j + 1] = a[j]
        j = j - 1
      end
      a[j + 1] = v
    end
  end
  local width = RUN
  while width < n do
    local k = 1
    for lo = 1, n, 2 * width do
      local mid = lo + width
      local hi = lo + 2 * width
      if mid > n + 1 then
        mid = n + 1
      end
      if hi > n + 1 then
        hi = n + 1
      end
      local i, j = lo, mid
      while i < mid and j < hi do
        local x, y = a[i], a[j]
        if less(y, x) then
          b[k] = y
          j = j + 1
        else
          b[k] = x
          i = i + 1
        end
        k = k + 1
      end
      while i < mid do
        b[k] = a[i]
        i = i + 1
        k = k + 1
      end
      while j < hi do
        b[k] = a[j]
        j = j + 1
        k = k + 1
      end
    end
    a, b = b, a
    width = width * 2
  end
  return a
end

return function(t, less)
  if type(t) ~= "table" then
    error("bad argument #1 to 'sort' (table expected, got " .. (t == nil and "no value" or type(t)) .. ")", 2)
  end
  if less ~= nil and type(less) ~= "function" then
    error("bad argument #2 to 'sort' (function expected, got " .. type(less) .. ")", 2)
  end
  local n = #t
  if n < 2 then
    return
  end
  local a, b = {}, {}
  for i = 1, n do
    a[i] = t[i]
  end
  local sorted
  if less then
    sorted = sort_with(less, a, b, n)
  else
    sorted = sort_default(a, b, n)
  end
  for i = 1, n do
    t[i] = sorted[i]
  end
end
