local function fib(n: number): number
  return (function()
    if (n < 2) then
      return n
    else
      return (fib((n - 1)) + fib((n - 2)))
    end
  end)()
end
local function main()
  println(fib(10))
end
