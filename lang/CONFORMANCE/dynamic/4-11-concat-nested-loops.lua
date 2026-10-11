local function main()
  local grid = ""
  do
    local __sb_1 = { grid }
    for r = 0, (3) - 1 do
      for c = 0, (3) - 1 do
        __sb_1[#__sb_1 + 1] = "#"
      end
      __sb_1[#__sb_1 + 1] = "/"
    end
    grid = table.concat(__sb_1)
  end
  println(grid)
  local log = ""
  for r = 0, (2) - 1 do
    do
      local __sb_2 = { log }
      for c = 0, (2) - 1 do
        __sb_2[#__sb_2 + 1] = "+"
      end
      log = table.concat(__sb_2)
    end
    println(log)
  end
end
