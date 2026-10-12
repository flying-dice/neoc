local function main()
  local function sum(n)
    return (function()
      if (n == 0) then
        return 0
      else
        return (n + sum((n - 1)))
      end
    end)()
  end
  println(sum(4))
end
