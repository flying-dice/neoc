--!strict
-- The agent's turn in a chat: one or more AG-UI runs, and what each event of them does to the chat.
--
-- A run is a POST of the conversation to the agent's endpoint, answered with a stream of events (`lib:agui`). Each
-- event becomes a change to the chat's items (timeline.luau) or its state. When a run ends having asked for vos
-- tools (the tools of the apps on this machine, offered in every run as AG-UI client tools), the turn runs them and
-- starts another run with their results, until the agent asks for no more.
local agui = require("lib:agui")
local app = require("vos:app")
local codec = require("./codec")
local random = require("lib:random")
local strings = require("lib:text")
local roster = require("./roster")
local timeline = require("./timeline")
local db = app.db

-- A turn runs vos tools and goes back to the agent at most this many times: an agent that asks for a tool in every
-- run would otherwise never give the person the chat back.
local MAX_ROUNDS = 16
-- The longest a run may take as a whole: an agent that works for longer than half an hour is stopped.
local RUN_TIMEOUT = 1800
-- How much of a tool's result is given to the agent (and kept).
local MAX_RESULT = 200000
-- How much one turn may bring into the chat, in bytes of what the agent sent: an agent cannot fill the database, or
-- make every later run send a conversation without end.
local MAX_TURN = 8 * 1024 * 1024
-- The largest state an agent may share, as JSON text.
local MAX_STATE = 1024 * 1024

-- What a run adds to the conversation it sends, besides the conversation: the answers to its interrupts, or what
-- the person did on a surface.
export type Extra = { resume: { any }?, forwarded: any }
-- The turn in progress in a chat: `stream` is the run being read, closed to stop it.
export type Turn = { stream: lib_agui_Run?, stopped: boolean }

local turn = {}

-- the id of an item as the agent knows it: its own for what it sent, ours for the rest
local function idOf(row: timeline.Row): string
  return if row.key ~= "" then row.key else `item-{row.id}`
end

-- The conversation as AG-UI messages. Activities, steps and the rest of what is only shown are left out, as the
-- protocol asks. A tool call belongs to an assistant message: the one before it, or one made for it.
local function history(chat: string): { lib_agui_Message }
  local messages: { any } = {}
  local calling: any = nil -- the assistant message the next tool call is added to
  for _, row in timeline.rows(chat) do
    if row.kind == "user" then
      table.insert(messages, { id = idOf(row), role = "user", content = row.text })
      calling = nil
    elseif row.kind == "assistant" then
      calling = { id = idOf(row), role = "assistant", content = row.text }
      table.insert(messages, calling)
    elseif row.kind == "tool" then
      local data = codec.decode(row.data) or {}
      -- a call still without a result is not sent: a model is not shown a call nobody answered
      if data.result ~= nil or data.error ~= nil then
        if not calling then
          calling = { id = `calls-{row.id}`, role = "assistant" }
          table.insert(messages, calling)
        end
        calling.tool_calls = calling.tool_calls or {}
        table.insert(calling.tool_calls, {
          id = idOf(row),
          type = "function",
          ["function"] = { name = row.name, arguments = if row.text == "" then "{}" else row.text },
        })
        table.insert(messages, {
          id = `result-{row.id}`,
          role = "tool",
          tool_call_id = idOf(row),
          content = data.result or "",
          error = data.error,
        })
      end
    end
  end
  return messages
end

type Offered = { tools: { lib_agui_Tool }, names: { [string]: boolean } }

-- the tools of the apps on this machine, as AG-UI client tools
local function offered(): Offered
  local out: Offered = { tools = {}, names = {} }
  for _, tool in app.tools.all() do
    -- this app has no tools of its own; were it given one, an agent could start a turn from inside a turn
    if tool.app ~= app.id then
      table.insert(out.tools, {
        name = tool.name,
        description = if tool.title then `{tool.title}. {tool.description}` else tool.description,
        parameters = tool.input_schema,
      })
      out.names[tool.name] = true
    end
  end
  return out
end

-- What one run needs to keep between its events.
type Run = {
  chat: string,
  ours: { [string]: boolean },
  -- the message a `*_CHUNK` event without an id belongs to: the last one that had an id
  textChunk: string?,
  toolChunk: string?,
  reasoningChunk: string?,
  thinking: number?,
  -- the run said how it ended (`RUN_FINISHED` or `RUN_ERROR`)
  ended: boolean,
  -- it ended asking the person something, or failed: the turn does not go on by itself
  halted: boolean,
  -- the last item of the chat when the run began: what the run names by an id is looked for after it, since an agent
  -- may use the same id again in a later run and mean a new thing
  since: number,
  -- the tool calls this run made, by item
  calls: { number },
  -- how much the turn has brought into the chat, in bytes (shared by the runs of the turn)
  budget: { spent: number },
}

local function saveState(chat: string, state: any)
  local text = if state == nil then "" else codec.encode(state)
  if #text > MAX_STATE then
    -- kept as it was: a state this large is not sent back with every run
    timeline.add(
      chat,
      "error",
      { text = "The agent shared a state too large to keep (over 1 MB); the last one is kept." }
    )
    return
  end
  db:exec("update threads set state = :s where id = :id", { s = text, id = chat })
  app.emit("state", { chat = chat, state = text })
end

local function loadState(chat: string): any
  local row = db:one("select state from threads where id = :id", { id = chat })
  if not row or row.state == "" then
    return nil
  end
  return codec.decode(row.state)
end

-- the item of `kind` the agent calls `key`, made if this is the first the chat hears of it
local function ensure(run: Run, kind: timeline.Kind, key: string, fields: timeline.Fields?): number
  local found = timeline.find(run.chat, kind, key, run.since)
  if found then
    return found.id
  end
  local made: timeline.Fields = fields or {}
  made.key = key
  if made.done == nil then
    made.done = false
  end
  return timeline.add(run.chat, kind, made)
end

local function finish(run: Run, kind: timeline.Kind, key: string?)
  local found = if key then timeline.find(run.chat, kind, key, run.since) else nil
  if found then
    timeline.set(run.chat, found.id, { done = true })
  end
end

-- the data of a tool item, changed by `change`
local function toolData(run: Run, id: number, change: (data: { [string]: any }) -> ())
  local row = timeline.get(run.chat, id)
  local data = (if row then codec.decode(row.data) else nil) or {}
  change(data)
  timeline.set(run.chat, id, { data = codec.encode(data) })
end

local function toolItem(run: Run, key: string, name: string?): number
  local found = timeline.find(run.chat, "tool", key, run.since)
  if found then
    return found.id
  end
  local id = timeline.add(run.chat, "tool", {
    key = key,
    name = name or "",
    data = codec.encode { ours = run.ours[name or ""] == true },
    done = false,
  })
  table.insert(run.calls, id)
  return id
end

-- a snapshot of the whole conversation: each message is set on the item that has its id, or added
local function reconcile(run: Run, messages: { any })
  for _, message in messages do
    local role, id = message.role, message.id
    if role == "user" or role == "assistant" or role == "reasoning" then
      local text = if type(message.content) == "string" then message.content else ""
      local key = id
      -- a message of the person's was sent under our id for it
      local ours = if role == "user" then tonumber(string.match(id, "^item%-(%d+)$") or "") else nil
      if ours and timeline.get(run.chat, ours) then
        timeline.set(run.chat, ours, { text = text })
      elseif text ~= "" then
        timeline.set(run.chat, ensure(run, role, key), { text = text, done = true })
      end
      for _, call in message.tool_calls or {} do
        local item = toolItem(run, call.id, call["function"].name)
        timeline.set(run.chat, item, { name = call["function"].name, text = call["function"].arguments, done = true })
      end
    elseif role == "tool" then
      local found = timeline.find(run.chat, "tool", message.tool_call_id)
      if found then
        toolData(run, found.id, function(data)
          data.result = if type(message.content) == "string" then message.content else codec.encode(message.content)
          data.error = message.error
        end)
      end
    elseif role == "activity" then
      local item = ensure(run, "activity", id, { name = message.activity_type })
      timeline.set(run.chat, item, { name = message.activity_type, data = codec.encode(message.content), done = true })
    end
  end
end

-- One event of a run. Every event type of the protocol is here: what it does to the chat, or why it does nothing.
local handle: { [string]: (run: Run, event: lib_agui_Event) -> () } = {
  -- lifecycle
  RUN_STARTED = function() end, -- the turn began when the person wrote
  RUN_FINISHED = function(run, event)
    run.ended = true
    local outcome = event.outcome
    if outcome and outcome.type == "interrupt" then
      -- the agent waits for the person: nothing it asked for is run behind their back
      run.halted = true
      for _, interrupt in (outcome.interrupts or {}) :: { lib_agui_Interrupt } do
        timeline.add(run.chat, "interrupt", {
          key = interrupt.id,
          text = interrupt.message or "The agent is waiting for an answer.",
          name = interrupt.reason or "",
          data = codec.encode {
            status = "open",
            reason = interrupt.reason,
            response_schema = interrupt.response_schema,
            tool_call_id = interrupt.tool_call_id,
          },
          done = false,
        })
      end
    end
  end,
  RUN_ERROR = function(run, event)
    run.ended = true
    run.halted = true
    timeline.add(run.chat, "error", { text = event.message or "The run failed.", name = event.code or "" })
  end,
  STEP_STARTED = function(run, event)
    timeline.add(run.chat, "step", { key = event.step_name or "", name = event.step_name or "", done = false })
  end,
  STEP_FINISHED = function(run, event)
    finish(run, "step", event.step_name)
  end,

  -- what the agent says
  TEXT_MESSAGE_START = function(run, event)
    -- only an assistant's words are the agent's answer; a message of another role is shown as one all the same
    run.textChunk = event.message_id or run.textChunk
    ensure(run, "assistant", event.message_id or "", { name = event.name or "" })
  end,
  TEXT_MESSAGE_CONTENT = function(run, event)
    timeline.append(run.chat, ensure(run, "assistant", event.message_id or ""), event.delta or "")
  end,
  TEXT_MESSAGE_END = function(run, event)
    finish(run, "assistant", event.message_id)
  end,
  TEXT_MESSAGE_CHUNK = function(run, event)
    run.textChunk = event.message_id or run.textChunk
    if run.textChunk then
      timeline.append(run.chat, ensure(run, "assistant", run.textChunk, { name = event.name or "" }), event.delta or "")
    end
  end,

  -- the tools it uses
  TOOL_CALL_START = function(run, event)
    run.toolChunk = event.tool_call_id or run.toolChunk
    toolItem(run, event.tool_call_id or "", event.tool_call_name)
  end,
  TOOL_CALL_ARGS = function(run, event)
    timeline.append(run.chat, toolItem(run, event.tool_call_id or "", nil), event.delta or "")
  end,
  TOOL_CALL_END = function(run, event)
    -- the arguments are whole; the call itself is done when it has a result
  end,
  TOOL_CALL_CHUNK = function(run, event)
    run.toolChunk = event.tool_call_id or run.toolChunk
    if run.toolChunk then
      timeline.append(run.chat, toolItem(run, run.toolChunk, event.tool_call_name), event.delta or "")
    end
  end,
  TOOL_CALL_RESULT = function(run, event)
    -- the result of a call of this run, or of one an earlier run left open
    local key = event.tool_call_id or ""
    local earlier = if timeline.find(run.chat, "tool", key, run.since)
      then nil
      else timeline.find(run.chat, "tool", key)
    local id = if earlier then earlier.id else toolItem(run, key, nil)
    toolData(run, id, function(data)
      data.result = event.content or ""
    end)
    timeline.set(run.chat, id, { done = true })
  end,

  -- what it thinks
  REASONING_START = function(run, event)
    ensure(run, "reasoning", event.message_id or "")
  end,
  REASONING_MESSAGE_START = function(run, event)
    run.reasoningChunk = event.message_id or run.reasoningChunk
    ensure(run, "reasoning", event.message_id or "")
  end,
  REASONING_MESSAGE_CONTENT = function(run, event)
    timeline.append(run.chat, ensure(run, "reasoning", event.message_id or ""), event.delta or "")
  end,
  REASONING_MESSAGE_END = function() end, -- more messages may follow in the same reasoning: REASONING_END ends it
  REASONING_MESSAGE_CHUNK = function(run, event)
    run.reasoningChunk = event.message_id or run.reasoningChunk
    if run.reasoningChunk then
      timeline.append(run.chat, ensure(run, "reasoning", run.reasoningChunk), event.delta or "")
    end
  end,
  REASONING_END = function(run, event)
    finish(run, "reasoning", event.message_id)
  end,
  REASONING_ENCRYPTED_VALUE = function() end, -- a provider's opaque form of its reasoning: nothing to show
  -- the same, as agents sent it before the REASONING events
  THINKING_START = function(run, event)
    run.thinking = timeline.add(run.chat, "reasoning", { name = event.title or "", done = false })
  end,
  THINKING_TEXT_MESSAGE_START = function(run)
    run.thinking = run.thinking or timeline.add(run.chat, "reasoning", { done = false })
  end,
  THINKING_TEXT_MESSAGE_CONTENT = function(run, event)
    run.thinking = run.thinking or timeline.add(run.chat, "reasoning", { done = false })
    timeline.append(run.chat, run.thinking :: number, event.delta or "")
  end,
  THINKING_TEXT_MESSAGE_END = function() end,
  THINKING_END = function(run)
    if run.thinking then
      timeline.set(run.chat, run.thinking, { done = true })
      run.thinking = nil
    end
  end,

  -- the state it shares, and the conversation as it has it
  STATE_SNAPSHOT = function(run, event)
    saveState(run.chat, event.snapshot)
  end,
  STATE_DELTA = function(run, event)
    local state, problem = agui.apply_patch(loadState(run.chat) or {}, event.delta or {})
    if problem then
      -- a patch that does not fit the state we have: the state is as it was, and the next snapshot sets it right
      return
    end
    saveState(run.chat, state)
  end,
  MESSAGES_SNAPSHOT = function(run, event)
    reconcile(run, event.messages or {})
  end,

  -- what it is doing
  ACTIVITY_SNAPSHOT = function(run, event)
    local found = timeline.find(run.chat, "activity", event.message_id or "")
    if found and event.replace == false then
      return
    end
    local fields = { name = event.activity_type or "", data = codec.encode(event.content), done = true }
    if found then
      timeline.set(run.chat, found.id, fields)
    else
      fields.key = event.message_id or ""
      timeline.add(run.chat, "activity", fields)
    end
  end,
  ACTIVITY_DELTA = function(run, event)
    local found = timeline.find(run.chat, "activity", event.message_id or "")
    if not found then
      return -- a patch to an activity that was never drawn
    end
    local content = agui.apply_patch(codec.decode(found.data) or {}, event.patch or {})
    if content ~= nil then
      timeline.set(run.chat, found.id, { data = codec.encode(content) })
    end
  end,
  SUBAGENT_STARTED = function(run, event)
    ensure(run, "subagent", event.subagent_run_id or "", { name = event.name or "", text = event.description or "" })
  end,
  SUBAGENT_FINISHED = function(run, event)
    finish(run, "subagent", event.subagent_run_id)
  end,
  SUBAGENT_ERROR = function(run, event)
    local found = timeline.find(run.chat, "subagent", event.subagent_run_id or "")
    if found then
      timeline.set(run.chat, found.id, { data = codec.encode { error = event.message }, done = true })
    end
  end,

  -- its own events
  CUSTOM = function(run, event)
    timeline.add(run.chat, "custom", { name = event.name or "", data = codec.encode(event.value) })
  end,
  RAW = function() end, -- an event of the agent's own framework, passed through for whoever knows it: not shown
}

-- the arguments of a tool call as a table; text that is not a JSON object is a problem
local function arguments(text: string): ({ [string]: any }?, string?)
  if string.match(text, "^%s*$") then
    return {}, nil
  end
  local args = codec.decode(text)
  if type(args) ~= "table" then
    return nil, "The arguments are not a JSON object."
  end
  return args, nil
end

-- Run the vos tools this run asked for. Returns how many ran. Only the calls of this run: a call an earlier turn
-- left open (the person stopped it) is not run later.
local function runTools(chat: string, agent: roster.Agent, ours: { [string]: boolean }, calls: { number }): number
  local ran = 0
  for _, id in calls do
    local row = timeline.get(chat, id)
    local data = if row then codec.decode(row.data) or {} else nil
    if row and data and data.result == nil and data.error == nil and ours[row.name] then
      local args, problem = arguments(row.text)
      local result: any = nil
      if args then
        result, problem = app.tools.call(row.name, args, { id = agent.id, name = agent.name })
      end
      data.ours = true
      if problem then
        data.error = strings.clip(problem, MAX_RESULT)
        data.result = data.error
      else
        -- cut between characters: text cut inside one is not text, and would not be kept
        data.result = strings.clip(codec.encode(result), MAX_RESULT)
      end
      timeline.set(chat, row.id, { data = codec.encode(data), done = true })
      ran += 1
    end
  end
  return ran
end

-- the person's words for a failure of `lib:agui`
local function sentence(err: string): string
  local kind = agui.error_kind(err)
  local message = agui.error_message(err) or err
  if kind == "connect" then
    return `The agent's endpoint could not be reached: {message}`
  elseif kind == "status" then
    return `The agent's endpoint refused the run: {message}`
  elseif kind == "protocol" then
    return `The endpoint did not answer as an AG-UI agent does: {message}`
  elseif kind == "timed_out" then
    return `The agent stopped answering: {message}`
  end
  return `The run failed: {message}`
end

-- how much of the turn's allowance an event takes: its text, and what it carries
local function weight(event: lib_agui_Event): number
  local e = event :: any
  local size = 200 + #(if type(e.delta) == "string" then e.delta else "")
  for _, carried in
    { e.snapshot, e.content, e.value, e.messages, e.patch, if type(e.delta) == "table" then e.delta else nil }
  do
    size += #codec.encode(carried)
  end
  return size
end

-- How a run left the turn: `more` when it ended as a run does and the turn may go on with what it asked for.
type Left = { more: boolean, calls: { number } }

-- One run: send the conversation, and let every event do its work.
local function once(
  chat: string,
  agent: roster.Agent,
  tools: Offered,
  extra: Extra,
  current: Turn,
  budget: { spent: number }
): Left
  local stopped: Left = { more = false, calls = {} }
  local token, problem = roster.token(agent)
  if problem then
    timeline.add(chat, "error", { text = problem })
    return stopped
  end
  local since = timeline.last(chat)
  local stream, err = agui.run {
    url = agent.url,
    headers = if token then { Authorization = `Bearer {token}` } else nil,
    timeout = RUN_TIMEOUT,
    input = {
      thread_id = chat,
      run_id = random.uuid_v4(),
      messages = history(chat),
      tools = tools.tools,
      context = {},
      state = loadState(chat),
      forwarded_props = extra.forwarded,
      resume = extra.resume,
    },
  }
  if not stream then
    timeline.add(chat, "error", { text = sentence(err or "") })
    return stopped
  end
  current.stream = stream
  local run: Run =
    { chat = chat, ours = tools.names, ended = false, halted = false, since = since, calls = {}, budget = budget }
  -- the run is over when it says so: an agent that keeps the connection open after that is not waited for
  while not run.ended do
    local event, failed = stream:next()
    if failed then
      -- a run the person stopped fails to be read: that is what stopping it is
      if not current.stopped then
        timeline.add(chat, "error", { text = sentence(failed) })
      end
      stream:close()
      return stopped
    end
    if not event then
      break
    end
    budget.spent += weight(event)
    if budget.spent > MAX_TURN then
      stream:close()
      current.stream = nil
      timeline.add(chat, "error", { text = "The agent sent more than a turn may hold (8 MB), and was stopped." })
      return stopped
    end
    local work = handle[event.type]
    if work then
      work(run, event)
    end
  end
  stream:close()
  current.stream = nil
  if not run.ended then
    if not current.stopped then
      timeline.add(chat, "error", { text = "The agent's answer ended before it was finished." })
    end
    return stopped
  end
  return { more = not run.halted, calls = run.calls }
end

-- The agent's turn: runs until the agent has nothing more to ask of the vos tools. It adds to the chat as it goes
-- and returns when the turn is over; a failure is an `error` item in the chat, never an error raised.
function turn.take(chat: string, agent: roster.Agent, extra: Extra, current: Turn)
  local tools = offered()
  local budget = { spent = 0 }
  for _ = 1, MAX_ROUNDS do
    local left = once(chat, agent, tools, extra, current, budget)
    timeline.settle(chat)
    if not left.more or current.stopped then
      return
    end
    -- the next run of the turn carries the tools' results, not the answers again
    extra = { forwarded = nil, resume = nil }
    if runTools(chat, agent, tools.names, left.calls) == 0 then
      return
    end
  end
  timeline.add(chat, "error", { text = `The agent asked for tools {MAX_ROUNDS} times in a row, and was stopped.` })
end

return turn
