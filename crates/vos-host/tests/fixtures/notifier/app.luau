--!strict
-- Sample app for the notification journeys (client/journeys/notifications.md): each button starts background work
-- that raises a notification of its own a moment later, whether or not the window is still open. It shows what the
-- person sees when an app's backend calls `app.notify`: a banner from the shell with this app's name and icon.
local app = require("vos:app")
local ui = require("vos:ui")
local task = require("std:task")

app.manifest {
  id = "notifier",
  name = "Notifier",
  icon = "bell",
  tint = "pink",
  window = { w = 360, h = 320, minW = 320, minH = 240 },
}

local s = app.state { sent = 0 }

-- Every notification comes from the app's own background work, a moment after the tap: the window may have been
-- closed, or another app brought forward, by then (the person sees a banner only where it is not in front).
local DELAY = 2

local function later(title: string, body: string?, times: number?)
  task.spawn(function()
    task.wait(DELAY)
    for i = 1, times or 1 do
      app.notify { title = if times then `{title} {i}` else title, body = body }
      s.sent = s.sent + 1
    end
  end)
end

app.window("main", {
  title = "Notifier",
  render = function()
    return ui.VStack {
      frame = "fill",
      align = "center",
      spacing = 1,
      padding = 3,
      ui.Spacer {},
      ui.Text { key = "sent", tostring(s.sent), label = "Sent", style = "display" },
      ui.Button {
        key = "later",
        title = "Notify in a moment",
        onTap = function()
          later("Check finished", "Nothing needed your attention.")
        end,
      },
      ui.Button {
        key = "burst",
        title = "Notify four times",
        onTap = function()
          later("Message", nil, 4)
        end,
      },
      ui.Button {
        key = "long",
        title = "Notify with long text",
        onTap = function()
          later(
            "The nightly backup of the photo library, the documents folder and every project folder finished with warnings",
            string.rep("A file was skipped because it was in use. ", 20)
          )
        end,
      },
      ui.Button {
        key = "odd",
        title = "Notify with odd text",
        onTap = function()
          -- a line break, an override that would reverse what follows, a zero-width space, and markup
          later("Odd\ntitle \u{202E}gnp.exe\u{200B}", "first line\nsecond line <b>not bold</b>")
        end,
      },
      ui.Spacer {},
    }
  end,
})
