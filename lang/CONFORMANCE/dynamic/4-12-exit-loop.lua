local function skip(n)
  local s = ""
  local i = 0
  do
    local __sb_1 = { (s .. "") }
    while (i < n) do
      local __brk_1 = false
      repeat
        i = i + 1
        local __exit_1, __val_1 = (function()
          if (i == 2) then
            return 3
          end
          if (i == 4) then
            return 2
          end
          return false, i
        end)()
        if __exit_1 == 2 then
          do __brk_1 = true; break end
        elseif __exit_1 == 3 then
          break
        end
        local v = __val_1
        __sb_1[#__sb_1 + 1] = "a"
      until true
      if __brk_1 then break end
    end
    s = table.concat(__sb_1)
  end
  return s
end
local function deep(n)
  local s = ""
  local i = 0
  do
    local __sb_2 = { (s .. "") }
    while true do
      i = i + 1
      local __exit_3, __val_3 = (function()
        if (i < n) then
          local __exit_2, __val_2 = (function()
            if (i == 3) then
              return 2
            end
            return false, 1
          end)()
          if __exit_2 then
            return 2
          end
          return false, __val_2
        else
          if true then
            return 2
          end
          return false, 0
        end
      end)()
      if __exit_3 then
        break
      end
      local a = __val_3
      __sb_2[#__sb_2 + 1] = "t"
    end
    s = table.concat(__sb_2)
  end
  return s
end
local function inner_loop(n)
  local s = ""
  do
    local __sb_3 = { (s .. "") }
    for i = 0, (n) - 1 do
      local v = (function()
        local t = 0
        while true do
          t = t + 1
          if (t > 1) then
            break
          end
        end
        return t
      end)()
      __sb_3[#__sb_3 + 1] = "b"
    end
    s = table.concat(__sb_3)
  end
  return s
end
local function main()
  println(skip(9))
  println(skip(3))
  println(deep(10))
  println(deep(2))
  println(inner_loop(3))
end
