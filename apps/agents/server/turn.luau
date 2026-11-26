--!strict
-- The agent's turn in a chat: one or more AG-UI runs, and what each event of them does to the chat.
--
-- A run is a POST of the conversation to the agent's endpoint, answered with a stream of events (`lib:agui`). Each
-- event becomes a change to the chat's items (timeline.luau) or its state. When a run ends having asked for vos
-- tools (the tools of the apps on this machine, offered in every run as AG-UI client tools), the turn runs them and
-- starts another run with their results, until the agent asks for no more.
local agui = require("lib:agui")
local codec = require("./codec")
local random = require("lib:random")
local strings = require("lib:text")
local roster = require("./roster")
local timeline = require("./timeline")
local db = app.db

-- A turn runs vos tools and goes back to the agent at most this many times: an agent that asks for a tool in every
-- run would otherwise never give the person the chat back.
local MAX_ROUNDS = 16
-- The most tools of this machine one run may have run.
local MAX_CALLS = 32
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
export type Turn = { stream: mlua_Stream<lib_agui_Event, lib_agui_RunError>?, stopped: boolean }

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
  RPC.send_notification("_state", { chat = chat, state = text })
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
  -- the snapshot names the whole conversation, the earlier runs' messages too: each is looked for in all of the chat
  local whole = table.clone(run)
  whole.since = 0
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
        timeline.set(run.chat, ensure(whole, role, key), { text = text, done = true })
      end
      for _, call in message.tool_calls or {} do
        local item = toolItem(whole, call.id, call["function"].name)
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
      local item = ensure(whole, "activity", id, { name = message.activity_type })
      timeline.set(run.chat, item, { name = message.activity_type, data = codec.encode(message.content), done = true })
    end
  end
end

-- One event of a run. Every event type of the protocol is here: what it does to the chat, or why it does nothing.
local function handle(run: Run, event: lib_agui_Event)
  -- lifecycle
  if event.type == "RUN_STARTED" then
    -- the turn began when the person wrote
  elseif event.type == "RUN_FINISHED" then
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
  elseif event.type == "RUN_ERROR" then
    run.ended = true
    run.halted = true
    timeline.add(run.chat, "error", { text = event.message or "The run failed.", name = event.code or "" })
  elseif event.type == "STEP_STARTED" then
    timeline.add(run.chat, "step", { key = event.step_name or "", name = event.step_name or "", done = false })
  elseif event.type == "STEP_FINISHED" then
    finish(run, "step", event.step_name)

  -- what the agent says
  elseif event.type == "TEXT_MESSAGE_START" then
    -- only an assistant's words are the agent's answer; a message of another role is shown as one all the same
    run.textChunk = event.message_id or run.textChunk
    ensure(run, "assistant", event.message_id or "", { name = event.name or "" })
  elseif event.type == "TEXT_MESSAGE_CONTENT" then
    timeline.append(run.chat, ensure(run, "assistant", event.message_id or ""), event.delta or "")
  elseif event.type == "TEXT_MESSAGE_END" then
    finish(run, "assistant", event.message_id)
  elseif event.type == "TEXT_MESSAGE_CHUNK" then
    run.textChunk = event.message_id or run.textChunk
    if run.textChunk then
      timeline.append(run.chat, ensure(run, "assistant", run.textChunk, { name = event.name or "" }), event.delta or "")
    end

  -- the tools it uses
  elseif event.type == "TOOL_CALL_START" then
    run.toolChunk = event.tool_call_id or run.toolChunk
    toolItem(run, event.tool_call_id or "", event.tool_call_name)
  elseif event.type == "TOOL_CALL_ARGS" then
    timeline.append(run.chat, toolItem(run, event.tool_call_id or "", nil), event.delta or "")
  elseif event.type == "TOOL_CALL_END" then
    -- the arguments are whole; the call itself is done when it has a result
  elseif event.type == "TOOL_CALL_CHUNK" then
    run.toolChunk = event.tool_call_id or run.toolChunk
    if run.toolChunk then
      timeline.append(run.chat, toolItem(run, run.toolChunk, event.tool_call_name), event.delta or "")
    end
  elseif event.type == "TOOL_CALL_RESULT" then
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

  -- what it thinks
  elseif event.type == "REASONING_START" then
    ensure(run, "reasoning", event.message_id or "")
  elseif event.type == "REASONING_MESSAGE_START" then
    run.reasoningChunk = event.message_id or run.reasoningChunk
    ensure(run, "reasoning", event.message_id or "")
  elseif event.type == "REASONING_MESSAGE_CONTENT" then
    timeline.append(run.chat, ensure(run, "reasoning", event.message_id or ""), event.delta or "")
  elseif event.type == "REASONING_MESSAGE_END" then
    -- more messages may follow in the same reasoning: REASONING_END ends it
  elseif event.type == "REASONING_MESSAGE_CHUNK" then
    run.reasoningChunk = event.message_id or run.reasoningChunk
    if run.reasoningChunk then
      timeline.append(run.chat, ensure(run, "reasoning", run.reasoningChunk), event.delta or "")
    end
  elseif event.type == "REASONING_END" then
    finish(run, "reasoning", event.message_id)
  elseif event.type == "REASONING_ENCRYPTED_VALUE" then
    -- a provider's opaque form of its reasoning: nothing to show
    -- the same, as agents sent it before the REASONING events
  elseif event.type == "THINKING_START" then
    run.thinking = timeline.add(run.chat, "reasoning", { name = event.title or "", done = false })
  elseif event.type == "THINKING_TEXT_MESSAGE_START" then
    run.thinking = run.thinking or timeline.add(run.chat, "reasoning", { done = false })
  elseif event.type == "THINKING_TEXT_MESSAGE_CONTENT" then
    run.thinking = run.thinking or timeline.add(run.chat, "reasoning", { done = false })
    timeline.append(run.chat, run.thinking :: number, event.delta or "")
  elseif event.type == "THINKING_TEXT_MESSAGE_END" then
    -- nothing to do
  elseif event.type == "THINKING_END" then
    if run.thinking then
      timeline.set(run.chat, run.thinking, { done = true })
      run.thinking = nil
    end

  -- the state it shares, and the conversation as it has it
  elseif event.type == "STATE_SNAPSHOT" then
    saveState(run.chat, event.snapshot)
  elseif event.type == "STATE_DELTA" then
    local state, problem = agui.apply_patch(loadState(run.chat) or {}, event.delta or {})
    if problem then
      -- a patch that does not fit the state we have: the state is as it was, and the next snapshot sets it right
      return
    end
    saveState(run.chat, state)
  elseif event.type == "MESSAGES_SNAPSHOT" then
    reconcile(run, event.messages or {})

  -- what it is doing
  elseif event.type == "ACTIVITY_SNAPSHOT" then
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
  elseif event.type == "ACTIVITY_DELTA" then
    local found = timeline.find(run.chat, "activity", event.message_id or "")
    if not found then
      return -- a patch to an activity that was never drawn
    end
    local content = agui.apply_patch(codec.decode(found.data) or {}, event.patch or {})
    if content ~= nil then
      timeline.set(run.chat, found.id, { data = codec.encode(content) })
    end
  elseif event.type == "SUBAGENT_STARTED" then
    ensure(run, "subagent", event.subagent_run_id or "", { name = event.name or "", text = event.description or "" })
  elseif event.type == "SUBAGENT_FINISHED" then
    finish(run, "subagent", event.subagent_run_id)
  elseif event.type == "SUBAGENT_ERROR" then
    local found = timeline.find(run.chat, "subagent", event.subagent_run_id or "", run.since)
    if found then
      timeline.set(run.chat, found.id, { data = codec.encode { error = event.message }, done = true })
    end

  -- its own events
  elseif event.type == "CUSTOM" then
    timeline.add(run.chat, "custom", { name = event.name or "", data = codec.encode(event.value) })
  elseif event.type == "RAW" then
    -- an event of the agent's own framework, passed through for whoever knows it: not shown
  end
end

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
local function runTools(
  chat: string,
  agent: roster.Agent,
  ours: { [string]: boolean },
  calls: { number },
  current: Turn,
  budget: { spent: number }
): number
  local ran = 0
  for _, id in calls do
    -- a stop during one tool is not followed by the next
    if current.stopped then
      break
    end
    local row = timeline.get(chat, id)
    local data = if row then codec.decode(row.data) or {} else nil
    if row and data and data.result == nil and data.error == nil and ours[row.name] then
      local args, problem = arguments(row.text)
      local result: any = nil
      if ran >= MAX_CALLS then
        problem = `The agent asked for more than {MAX_CALLS} tools in one go; this one was not run.`
      elseif budget.spent > MAX_TURN then
        problem = "The turn already holds as much as it may (8 MB); this tool was not run."
      elseif args then
        local answer, failure = app.tools.call(row.name, { id = agent.id, name = agent.name }, args)
        result, problem = answer, if failure then failure.message else nil
      end
      data.ours = true
      local text = if problem then "" else codec.encode(result)
      if not problem and #text > MAX_RESULT then
        -- not cut short: half a result reads as a whole one
        problem = "The tool's result was too large to give to the agent."
      end
      if problem then
        -- cut between characters: text cut inside one is not text, and would not be kept
        data.error = strings.clip(problem, 2000)
        data.result = data.error
      else
        data.result = text
      end
      -- what a tool gives is part of what the turn brings into the chat, as the agent's own events are
      budget.spent += 200 + #data.result
      timeline.set(chat, row.id, { data = codec.encode(data), done = true })
      ran += 1
    end
  end
  return ran
end

-- the person's words for a failure of `lib:agui`
local function sentence(err: lib_agui_RunError?): string
  if not err then
    return "The run failed."
  end
  local kind, message = err.kind, err.message
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

-- how much of the turn's allowance an event takes: all of it, whatever field the agent put its text in
local function weight(event: lib_agui_Event): number
  return 200 + #codec.encode(event)
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
  local stream, err = agui.run(agent.url, {
    thread_id = chat,
    run_id = random.uuid_v4(),
    messages = history(chat),
    tools = tools.tools,
    context = {},
    state = loadState(chat),
    forwarded_props = extra.forwarded,
    resume = extra.resume,
  }, {
    headers = if token then { Authorization = `Bearer {token}` } else nil,
    timeout = RUN_TIMEOUT,
  })
  if not stream then
    timeline.add(chat, "error", { text = sentence(err) })
    return stopped
  end
  current.stream = stream
  local run: Run = { chat = chat, ours = tools.names, ended = false, halted = false, since = since, calls = {} }
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
    handle(run, event)
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
    if runTools(chat, agent, tools.names, left.calls, current, budget) == 0 or current.stopped then
      return
    end
  end
  timeline.add(chat, "error", { text = `The agent asked for tools {MAX_ROUNDS} times in a row, and was stopped.` })
end

return turn
