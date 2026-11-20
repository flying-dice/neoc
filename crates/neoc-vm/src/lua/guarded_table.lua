-- `table.insert`, `table.remove`, `table.sort` and `table.move` for a sealed environment (`script_env::freeze`).
--
-- LuaJIT's table functions store with raw sets, which skip a proxy's `__newindex`: `table.insert(string, "x")`
-- would write into the read-only proxy that stands for a library. These versions refuse a frozen table first and
-- otherwise are the engine's own functions, called with the same arguments.

local guarded, insert, remove, sort, move = ...

local function check(t)
  if guarded[t] then
    error("attempt to modify a read-only table", 3)
  end
end

return {
  insert = function(t, ...)
    check(t)
    return insert(t, ...)
  end,
  remove = function(t, ...)
    check(t)
    return remove(t, ...)
  end,
  sort = function(t, ...)
    check(t)
    return sort(t, ...)
  end,
  -- `table.move(a1, f, e, t [, a2])` writes into `a2` when it is given and into `a1` when it is not
  move = function(a1, f, e, t, a2)
    if a2 == nil then
      check(a1)
    else
      check(a2)
    end
    return move(a1, f, e, t, a2)
  end,
}
