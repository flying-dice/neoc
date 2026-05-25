local Dir = {}
Dir.__index = Dir
function Dir.name(self)
  return (function()
    local __s = self
    if __s.tag == "N" then
      return "north"
    end
    if __s.tag == "S" then
      return "south"
    end
    if __s.tag == "E" then
      return "east"
    end
    if __s.tag == "W" then
      return "west"
    end
  end)()
end
local function main()
  local d = setmetatable({ _enum = "Dir", tag = "E" }, Dir)
  println(d:name())
end
