local function label()
  return 1
end
local function main()
  local fact, is_even, is_odd, label
  function fact(n)
    return (function()
      if (n < 2) then
        return 1
      else
        return (n * fact((n - 1)))
      end
    end)()
  end
  function is_even(n)
    return (function()
      if (n == 0) then
        return 1
      else
        return is_odd((n - 1))
      end
    end)()
  end
  function is_odd(n)
    return (function()
      if (n == 0) then
        return 0
      else
        return is_even((n - 1))
      end
    end)()
  end
  function label()
    return 2
  end
  println(label())
  println(fact(5))
  println(is_even(10))
end
