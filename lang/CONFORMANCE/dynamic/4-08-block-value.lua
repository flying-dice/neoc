local function main()
  local x = (function()
    local a = 2
    local b = 3
    return (a * b)
  end)()
  println(x)
end
