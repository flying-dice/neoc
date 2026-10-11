local function report(n)
  if (n > 1) then
    println("high")
    return
  end
  println("low")
end
local function main()
  report(0)
  report(2)
end
