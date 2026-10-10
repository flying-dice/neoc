local Signal = {}
Signal.__index = Signal
function Signal.clone(self)
  if self.__tag == "Pulse" then
    return { __enum = "Signal", __tag = "Pulse", self[1] }
  end
  if self.__tag == "Idle" then
    return { __enum = "Signal", __tag = "Idle" }
  end
end
function Signal.eq(a, b)
  if a.__enum ~= b.__enum or a.__tag ~= b.__tag then
    return false
  end
  if a.__tag == "Pulse" then
    return a[1] == b[1]
  end
  return true
end
function Signal.debug(self)
  if self.__tag == "Pulse" then
    return "Pulse(" .. tostring(self[1]) .. ")"
  end
  if self.__tag == "Idle" then
    return "Idle"
  end
end
function Signal.display(self)
  if self.__tag == "Pulse" then
    return tostring(self[1])
  end
  if self.__tag == "Idle" then
    return "Idle"
  end
end
Signal.__traits = { "Clone", "Debug", "Display", "Eq" }
local function main()
  local a = setmetatable({ __enum = "Signal", __tag = "Pulse", 7 }, Signal)
  local b = a:clone()
  if a:eq(b) then
    println("clone-eq")
  end
  local c = setmetatable({ __enum = "Signal", __tag = "Idle" }, Signal)
  if (not a:eq(c)) then
    println("tag-ne")
  end
  local d = setmetatable({ __enum = "Signal", __tag = "Pulse", 9 }, Signal)
  if (not a:eq(d)) then
    println("payload-ne")
  end
  println(a:debug())
  println(c:debug())
  println(a:display())
  println(c:display())
end
