local function pick(big)
  local r
  if big then
    r = 100
  else
    r = 1
  end
  return r
end
local function main()
  println(pick(true))
  println(pick(false))
end
