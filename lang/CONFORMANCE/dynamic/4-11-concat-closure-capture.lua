local function main()
  local s = ""
  local peek = function() return s end
  for i = 0, (3) - 1 do
    s = (s .. "z")
    println(peek())
  end
end
