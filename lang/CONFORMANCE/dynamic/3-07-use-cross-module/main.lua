local Circle = require("geo.shape").Circle
local Kind = require("geo.shape").Kind
local plus = require("util").add
local Counter = require("util").Counter
local function main()
  println(plus(2, 3))
  local c = Counter.new({ n = 4 })
  println(c:bump())
  local circle = Circle.new({ r = 3 })
  println(circle:area())
  local rank = (function()
    local __s = circle:kind()
    if __s.__tag == "Small" then
      return 1
    end
    if __s.__tag == "Large" then
      return 2
    end
  end)()
  println(rank)
end
return { main = main }
