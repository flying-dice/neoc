--!strict
-- The Terminal app's backend: it owns the pseudo-terminals (sessions.luau, on `vnd:portable_pty`). A page opens a
-- session, sends what is typed, and hears the shell's output as events.
--
-- The shell is the person's own (`$SHELL`, else the one of the password database), started as a login shell in their
-- home folder with `TERM=xterm-256color`, as the user the host runs as. Whoever can open this app has that user's
-- shell: that is every paired device. No tool is offered to agents.
--
-- (a) What a session belongs to: the page that opened it, and nothing else. `open` answers with a random id (16 bytes
--     from the system), and every call and event names it. `app.emit` reaches every open window of the app on every
--     paired device: the page of another paired device receives the events of this session, id and output included,
--     and ignores them, because it shows only the session it opened itself. So the id keeps sessions apart; it is not a
--     secret from a paired device, and it need not be: a paired device can open a shell of its own.
--
-- (b) A session whose page goes away. The page closes it when the host removes the page (`host.onTeardown`: the
--     window was closed). For the rest (the browser was closed, the page reloaded, the network dropped) the page is
--     listened for: every call that names the session counts, and a page calls `ack` every 15 seconds at least. A
--     session not heard for 3 minutes is ended and its shell with it. (Not sooner: a browser lets a page in a
--     background tab run a timer only once a minute.) Unloading the app, or stopping the host, ends every session.
--
-- (c) A page that reloads starts a new session: a page cannot keep the id across a reload (its frame has no storage),
--     and the session it left is ended by (b). A page that only lost its connection keeps its session: when the
--     connection is back it asks for what it missed (`replay`, from the last 384 KiB of output) and goes on. If it was
--     away for more than the 3 minutes, the session is gone and the page says so.
--
-- (d) Limits. 8 sessions at once; when all are taken, a new one takes the place of the one heard least recently if that
--     one was silent for 45 seconds, and is refused otherwise. A write is at most 64 KiB, and a page sends one at a
--     time. Output is sent in events of at most 32 KiB (an event is JSON of at most 64 KiB, and the bytes are base64),
--     each with the offset of its first byte. The host does not queue an event for a client that is more than 1 MiB
--     behind, so an event can be lost: the page sees the gap in the offsets (or, for the last event, in the answer to
--     its `ack`) and asks for it again with `replay`. The page acknowledges what it has drawn, and a session reads no
--     more of its terminal while it is 256 KiB ahead of that: a command that prints without end waits in its own write,
--     the host holds at most the 384 KiB kept for `replay` per session, and every read is a wait other calls run in.
local rpc = require("../.app/rpc")
local sessions = require("./sessions")

local handlers: rpc.Handlers = {
  open = function(ctx, params)
    local session, problem = sessions.start(params.rows, params.cols)
    return if session then { session = session } else ctx:fail(problem or "The shell could not be started.")
  end,

  write = function(ctx, params)
    local problem = sessions.write(params.session, params.data)
    return if problem then ctx:fail(problem) else nil
  end,

  resize = function(ctx, params)
    local problem = sessions.resize(params.session, params.rows, params.cols)
    return if problem then ctx:fail(problem) else nil
  end,

  ack = function(ctx, params)
    local status, problem = sessions.ack(params.session, params.offset)
    return status or ctx:fail(problem or "This session has ended.")
  end,

  replay = function(ctx, params)
    local replay, problem = sessions.replay(params.session, params.from)
    return replay or ctx:fail(problem or "This session has ended.")
  end,

  close = function(_ctx, params)
    sessions.close(params.session)
    return nil
  end,
}

return handlers
