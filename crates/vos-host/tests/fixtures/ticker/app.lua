-- Sample app for the background-work tests: a counter that a task of the app raises on its own, every 0.2 s, for
-- as long as the app is loaded. It shows what AGOS-52 and AGOS-54 promise: the work starts at load without
-- holding it up (`task.spawn` at the top level), it goes on when the window is closed, it re-renders every open
-- window by itself, and it stops when the app is unloaded.
local app = require("vos:app")
local ui = require("vos:ui")
local task = require("std:task")

app.manifest {
  id = "ticker",
  name = "Ticker",
  icon = "clock",
  tint = "teal",
  window = { w = 360, h = 280, minW = 320, minH = 240 },
}

local s = app.state { ticks = 0, running = true }

-- the background loop: it is the app's own, so it is stopped when the app is unloaded
task.spawn(function()
  while true do
    task.wait(0.2)
    if s.running then
      s.ticks = s.ticks + 1
    end
  end
end)

app.window("main", {
  title = "Ticker",
  render = function()
    local status = "Counting"
    if not s.running then
      status = "Paused"
    end
    return ui.VStack {
      frame = "fill",
      align = "center",
      spacing = 2,
      padding = 3,
      ui.Spacer {},
      ui.Text { key = "ticks", tostring(s.ticks), label = "Ticks", style = "display" },
      ui.Text { key = "status", status, style = "body", foreground = "secondary" },
      ui.HStack {
        spacing = 1,
        ui.Button {
          key = "toggle",
          title = s.running and "Pause" or "Resume",
          onTap = function()
            s.running = not s.running
          end,
        },
        ui.Button {
          key = "reset",
          title = "Reset",
          onTap = function()
            s.ticks = 0
          end,
        },
        ui.Button {
          key = "fail",
          title = "Fail",
          role = "destructive",
          onTap = function()
            error("the Fail button always fails")
          end,
        },
      },
      ui.Spacer {},
    }
  end,
})

app.tool {
  name = "ticks",
  title = "Ticks",
  readOnly = true,
  description = "How many ticks the background loop has counted.",
  input = {},
  run = function()
    return { ticks = s.ticks, running = s.running }
  end,
}
