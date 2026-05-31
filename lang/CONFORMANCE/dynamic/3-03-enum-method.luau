type Dir = { __enum: "Dir", __tag: "N" } | { __enum: "Dir", __tag: "S" } | { __enum: "Dir", __tag: "E" } | { __enum: "Dir", __tag: "W" }

local Dir = {}
Dir.__index = Dir
function Dir.name(self): string
  return (function()
    local __s = self
    if __s.__tag == "N" then
      return "north"
    end
    if __s.__tag == "S" then
      return "south"
    end
    if __s.__tag == "E" then
      return "east"
    end
    if __s.__tag == "W" then
      return "west"
    end
  end)()
end
local function main()
  local d = setmetatable({ __enum = "Dir", __tag = "E" }, Dir)
  println(d:name())
end
