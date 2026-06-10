type Count = number

local function double(c: Count): Count
  return (c + c)
end
local function main()
  println(double(7))
  println(double(21))
end
