--!strict
-- The sessions: each a pseudo-terminal with the person's shell on it (`vnd:portable_pty`), the output it has sent, and
-- when its page was last heard. The handlers (main.luau) are thin over this; the decisions are written at the top of
-- main.luau, and the numbers are here.
local base64 = require("lib:base64")
local env = require("std:env")
local portable_pty = require("vnd:portable_pty")
local random = require("lib:random")
local task = require("std:task")
local time = require("std:time")
local tracing = require("lib:tracing")

local sessions = {}

-- how many sessions are open at once
local MAX_SESSIONS = 8
-- The most output in one `output` event, in bytes. An event is JSON of at most 64 KiB: 32 KiB is 43 692 characters
-- of base64, which leaves room for the rest of the event.
local CHUNK = 32 * 1024
-- how far the output sent may run ahead of what the page says it has drawn, before the session stops reading
local WINDOW = 256 * 1024
-- The output kept for `replay`. More than WINDOW and one CHUNK, which is the most a page that acknowledges can be
-- behind: such a page can always be given everything it missed.
local SCROLLBACK = 384 * 1024
-- how often a session that waits for its page looks again, in seconds
local WINDOW_POLL = 0.02
-- A session whose page was not heard for this long is ended, in seconds. A page says it is there every 15 seconds; a
-- browser lets a page in a background tab run a timer only once a minute, and three minutes is two of those missed.
local IDLE = 180
-- how often the sessions are looked over for one to end, in seconds
local SWEEP = 10
-- When every session is taken, a new one may take the place of the one heard least recently, if that one was not
-- heard for this long (three of its page's 15 seconds): its page is almost certainly gone.
local PRESUMED_GONE = 45
-- how long the last output of a shell that has ended is waited for before its terminal is closed, in seconds
local DRAIN = 0.5

type Session = {
  id: string,
  pty: vnd_portable_pty_Pty,
  -- the count of bytes sent as `output` notifications, and of those the page says it has drawn
  sent: number,
  acked: number,
  -- the output kept for `replay`: pieces in order, the offset of the first, and their total length
  kept: { string },
  keptFrom: number,
  keptBytes: number,
  -- when the page was last heard
  heard: std_time_Instant,
  -- how the shell ended, once it has
  exit: terminal_Exit?,
  -- every byte of output was read (the terminal's other end is closed)
  drained: boolean,
  -- the session was ended here: its tasks stop
  closed: boolean,
}

local open: { [string]: Session } = {}

local function count(): number
  local n = 0
  for _ in open do
    n += 1
  end
  return n
end

local function silentFor(session: Session): number
  return session.heard:elapsed():as_secs_f64()
end

local function finish(session: Session)
  session.closed = true
  session.pty:close()
  open[session.id] = nil
end

local function keep(session: Session, data: string)
  table.insert(session.kept, data)
  session.keptBytes += #data
  while session.keptBytes - #session.kept[1] >= SCROLLBACK do
    local dropped = table.remove(session.kept, 1) :: string
    session.keptBytes -= #dropped
    session.keptFrom += #dropped
  end
end

-- The session's reader: the terminal's output to the page, a piece at a time and in order. It reads only while the
-- page keeps up, so a command that prints without end fills the terminal's own buffer and waits there.
local function pump(session: Session)
  while not session.closed do
    if session.sent - session.acked >= WINDOW then
      task.wait(WINDOW_POLL)
      continue
    end
    local data, err = session.pty:read(CHUNK)
    if not data then
      if err then
        tracing.warn("could not read the terminal's output", { error = err.message, session = session.id })
      end
      break
    end
    -- from the read to the send nothing waits: the notifications leave in the order of the output
    keep(session, data)
    RPC.send_notification("output", { session = session.id, offset = session.sent, data = base64.encode(data) })
    session.sent += #data
  end
  session.drained = true
end

-- The session's watcher: the shell's end. What the shell wrote last is given a moment to be read; then the terminal
-- is closed (a job the shell left behind loses its terminal, as it does when any terminal closes) and the page is
-- told. The session stays, without a process, until its page closes it or stops being heard: the page may still ask
-- for output it missed.
local function watch(session: Session)
  local status, err = session.pty:wait()
  if session.closed then
    return
  end
  local waited = 0
  while not session.drained and waited < DRAIN do
    waited += task.wait(WINDOW_POLL)
  end
  session.pty:close()
  if not status then
    tracing.warn(
      "could not tell how the shell ended",
      { error = if err then err.message else "unknown", session = session.id }
    )
  end
  local exit: terminal_Exit = if status
    then { code = status.exit_code, signal = status.signal }
    else { code = 1, signal = if err then err.message else "unknown" }
  session.exit = exit
  RPC.send_notification("exit", { session = session.id, offset = session.sent, code = exit.code, signal = exit.signal })
end

-- End the sessions whose page is gone. The app's own task: it starts at load and stops when the app is unloaded
-- (and the terminals with it: the engine closes them).
task.spawn(function()
  while true do
    task.wait(SWEEP)
    for _, session in open do
      if silentFor(session) > IDLE then
        tracing.info("ended a terminal session that was not heard from", { session = session.id, seconds = IDLE })
        finish(session)
      end
    end
  end
end)

-- room for one more session: there is some, or the session heard least recently is presumed gone and ended
local function makeRoom(): boolean
  if count() < MAX_SESSIONS then
    return true
  end
  local quietest: Session? = nil
  for _, session in open do
    if quietest == nil or silentFor(session) > silentFor(quietest) then
      quietest = session
    end
  end
  if quietest and silentFor(quietest) > PRESUMED_GONE then
    finish(quietest)
    return true
  end
  return false
end

-- Start the person's shell. Answers with the session's id, or with a sentence for the person.
function sessions.start(rows: number, cols: number): (string?, string?)
  if not makeRoom() then
    return nil, `{MAX_SESSIONS} terminals are open already. Close one first.`
  end
  -- 16 random bytes from the system: not a number anyone guesses
  local id = random.token_urlsafe(16)
  if not id then
    return nil, "The system gave no random number for the session."
  end
  -- no `program`: the person's own shell, as a login shell. In their home folder; `env` is added to the host's own.
  local pty, err = portable_pty.spawn_command {
    rows = rows,
    cols = cols,
    cwd = env.home_dir(),
    env = { TERM = "xterm-256color" },
  }
  if not pty then
    tracing.warn("could not start the shell", { error = if err then err.message else "no terminal" })
    return nil, "The shell could not be started."
  end
  local session: Session = {
    id = id,
    pty = pty,
    sent = 0,
    acked = 0,
    kept = {},
    keptFrom = 0,
    keptBytes = 0,
    heard = time.now(),
    exit = nil,
    drained = false,
    closed = false,
  }
  open[id] = session
  task.spawn(pump, session)
  task.spawn(watch, session)
  return id, nil
end

-- The session a call names, and the page is heard.
local function heard(id: string): Session?
  local session = open[id]
  if session then
    session.heard = time.now()
  end
  return session
end

local ENDED = "This session has ended."

-- Type `data` (base64 of bytes). Answers with a sentence when it could not.
function sessions.write(id: string, data: string): string?
  local session = heard(id)
  if not session then
    return ENDED
  end
  if session.exit then
    return "The shell has exited."
  end
  local bytes = base64.decode(data)
  if not bytes then
    return "What was typed did not arrive whole."
  end
  local written, err = session.pty:write(bytes)
  if not written then
    return if err and err.kind == "closed" then "The shell has exited." else "The shell did not take what was typed."
  end
  return nil
end

function sessions.resize(id: string, rows: number, cols: number): string?
  local session = heard(id)
  if not session then
    return ENDED
  end
  -- a shell that has ended has no size to change: nothing to say
  session.pty:resize(rows, cols)
  return nil
end

function sessions.ack(id: string, offset: number): (terminal_Status?, string?)
  local session = heard(id)
  if not session then
    return nil, ENDED
  end
  session.acked = math.max(session.acked, math.min(offset, session.sent))
  return { offset = session.sent, exit = session.exit }, nil
end

function sessions.replay(id: string, from: number): (terminal_Replay?, string?)
  local session = heard(id)
  if not session then
    return nil, ENDED
  end
  local start = math.clamp(from, session.keptFrom, session.sent)
  local pieces: { string } = {}
  local at = session.keptFrom
  for _, piece in session.kept do
    local after = at + #piece
    if after > start then
      table.insert(pieces, if at >= start then piece else string.sub(piece, start - at + 1))
    end
    at = after
  end
  -- what is replayed is drawn at once: the page has it
  session.acked = math.max(session.acked, start)
  return { from = start, data = base64.encode(table.concat(pieces)), exit = session.exit }, nil
end

function sessions.close(id: string)
  local session = open[id]
  if session then
    finish(session)
  end
end

return sessions
