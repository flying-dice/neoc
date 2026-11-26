--!strict
-- The backend of the fixture app of the notification journeys (`e2e/notifications.journeys.spec.ts`): each button of
-- the page starts background work that raises a notification of its own a moment later, whether or not the window is
-- still open. It shows what the person sees when an app's backend calls `app.notify`: a banner from the shell with
-- this app's name and icon.
local task = require("std:task")

local sent = 0

-- Every notification comes from the app's own background work, a moment after the tap: the window may have been
-- closed, or another app brought forward, by then (the person sees a banner only where it is not in front).
local DELAY = 2

local function later(title: string, body: string?, times: number?)
  task.spawn(function()
    task.wait(DELAY)
    for i = 1, times or 1 do
      app.notify { title = if times then `{title} {i}` else title, body = body }
      sent += 1
    end
    RPC.send_notification("sent_changed", { count = sent })
  end)
end

RPC.on_request("_sent", function(): notifier_SentResult
  return sent
end)

RPC.on_request("_notify", function(params: notifier_NotifyParams): notifier_NotifyResult
  if params.kind == "later" then
    later("Check finished", "Nothing needed your attention.")
  elseif params.kind == "burst" then
    later("Message", nil, 4)
  elseif params.kind == "long" then
    later(
      "The nightly backup of the photo library, the documents folder and every project folder finished with warnings",
      string.rep("A file was skipped because it was in use. ", 20)
    )
  else
    -- a line break, an override that would reverse what follows, a zero-width space, and markup
    later("Odd\ntitle \u{202E}gnp.exe\u{200B}", "first line\nsecond line <b>not bold</b>")
  end
  return "started"
end)
