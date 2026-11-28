--!strict
-- The agent's turn in a chat: one prompt (ACP's `session/prompt`, through `vnd:agent_client_protocol`), and what each
-- thing the agent does in it does to the chat's items (timeline.luau).
--
-- What the agent says is the answer, what it thinks is reasoning, each tool call is a tool item (changed by every
-- update of it), its plan is one plan item for the turn (replaced whole by each plan it sends), and a permission it
-- asks for is a permission item the person answers (main.luau's `_answer`). The rest of what it sends (the commands
-- it has, the mode it is in) is not shown.
local codec = require("./codec")
local diff = require("lib:diff")
local link = require("./link")
local strings = require("lib:text")
local timeline = require("./timeline")
local tracing = require("lib:tracing")

-- How much one turn may bring into the chat, in bytes of what the agent sent: an agent cannot fill the database.
local MAX_TURN = 8 * 1024 * 1024
-- The longest text kept of one piece of a tool call (a text it produced, a side of a change to a file).
local MAX_TEXT = 200000
-- The largest image kept in the chat, as base64: a larger one is named, not shown.
local MAX_IMAGE = 1024 * 1024

-- The turn in progress in a chat: `handle` is the prompt's turn once it is sent (permissions are answered on it), and
-- `stopped` says the person stopped it.
export type Turn = { handle: vnd_agent_client_protocol_Turn?, stopped: boolean }
-- How a turn ended: `lost` when the connection turned out closed before the agent did anything, and the caller said
-- it may connect again and send the prompt once more; `over` however else it ended.
export type Outcome = "over" | "lost"

-- What one turn keeps between the agent's updates.
type Run = {
  chat: string,
  -- the last item of the chat when the turn began: a tool call id is looked for after it, since an agent may use the
  -- same id again in a later turn and mean a new thing
  since: number,
  -- the answer and the reasoning being written: a piece of either goes on with the last one, until something else
  -- comes between them
  speaking: number?,
  thinking: number?,
  -- the plan of this turn
  plan: number?,
}

local turn = {}

local function clip(text: string): string
  return strings.clip(text, MAX_TEXT, "…")
end

-- A piece of content as the window shows it: text, an image, a link or an embedded resource. What it cannot show (a
-- sound, an image too large) is a line of text that says what it is.
local function piece(block: vnd_agent_client_protocol_ContentBlock): { [string]: any }
  if block.type == "text" then
    return { type = "text", text = clip(block.text) }
  elseif block.type == "image" then
    if #block.data > MAX_IMAGE then
      return { type = "text", text = `[An image too large to show here ({block.mime_type})]` }
    end
    return { type = "image", mime_type = block.mime_type, data = block.data }
  elseif block.type == "resource_link" then
    return { type = "link", uri = block.uri, name = block.title or block.name }
  elseif block.type == "resource" then
    local resource = block.resource
    if resource.text then
      return { type = "resource", uri = resource.uri, text = clip(resource.text) }
    end
    return { type = "link", uri = resource.uri, name = resource.uri }
  end
  return { type = "text", text = `[A sound clip ({block.mime_type})]` }
end

-- What a tool call produced, as the window shows it: a change to a file is a unified diff.
local function produced(content: { vnd_agent_client_protocol_ToolCallContent }): { any }
  local list: { any } = {}
  for _, entry in content do
    if entry.type == "content" then
      table.insert(list, piece(entry.content))
    elseif entry.type == "diff" then
      local old, new = clip(entry.old_text or ""), clip(entry.new_text)
      local patch = diff.format(diff.unified(old, new, { old_label = entry.path, new_label = entry.path }))
      table.insert(list, { type = "diff", path = entry.path, patch = patch, new_file = entry.old_text == nil })
    elseif entry.type == "terminal" then
      table.insert(list, { type = "terminal", terminal_id = entry.terminal_id })
    end
  end
  return list
end

-- The fields of a tool call an update may carry: those it has replace the call's, and the rest stay.
type ToolFields = {
  title: string?,
  kind: vnd_agent_client_protocol_ToolKind?,
  status: vnd_agent_client_protocol_ToolCallStatus?,
  content: { vnd_agent_client_protocol_ToolCallContent }?,
  locations: { vnd_agent_client_protocol_ToolCallLocation }?,
  raw_input: any?,
  raw_output: any?,
}

-- the tool item of the call `key` with what `fields` say of it, made if this is the first the chat hears of it
local function tool(run: Run, key: string, fields: ToolFields)
  local found = timeline.find(run.chat, "tool", key, run.since)
  local data: { [string]: any } = (if found then codec.decode(found.data) else nil) or {}
  if fields.kind ~= nil then
    data.kind = fields.kind
  end
  if fields.status ~= nil then
    data.status = fields.status
  end
  if fields.content ~= nil then
    data.content = produced(fields.content)
  end
  if fields.locations ~= nil then
    local places: { any } = {}
    for _, location in fields.locations do
      table.insert(places, { path = location.path, line = location.line })
    end
    data.locations = places
  end
  if fields.raw_input ~= nil then
    data.input = clip(codec.encode(fields.raw_input))
  end
  if fields.raw_output ~= nil then
    data.output = clip(codec.encode(fields.raw_output))
  end
  local status = data.status
  local done = status == "completed" or status == "failed"
  if found then
    timeline.set(run.chat, found.id, { name = fields.title, data = codec.encode(data), done = done })
  else
    -- something new comes between the answer (or the reasoning) and what follows it
    run.speaking, run.thinking = nil, nil
    timeline.add(
      run.chat,
      "tool",
      { key = key, name = fields.title or "A tool", data = codec.encode(data), done = done }
    )
  end
end

-- A piece of the agent's answer: text goes on with the answer being written; anything else is an item of its own.
local function say(run: Run, block: vnd_agent_client_protocol_ContentBlock)
  run.thinking = nil
  local shown = piece(block)
  if shown.type == "text" then
    if not run.speaking then
      run.speaking = timeline.add(run.chat, "assistant", { done = false })
    end
    timeline.append(run.chat, run.speaking :: number, shown.text)
    return
  end
  run.speaking = nil
  timeline.add(run.chat, "assistant", {
    name = shown.type,
    text = shown.name or shown.uri or "",
    data = codec.encode(shown),
  })
end

-- One update of the session. Every kind of update is here: what it does to the chat, or why it does nothing.
local function update(run: Run, change: vnd_agent_client_protocol_SessionUpdate)
  if change.type == "agent_message_chunk" then
    say(run, change.content)
  elseif change.type == "agent_thought_chunk" then
    run.speaking = nil
    local content = change.content
    -- a thought is text; anything else in it is not shown
    if content.type == "text" then
      if not run.thinking then
        run.thinking = timeline.add(run.chat, "reasoning", { done = false })
      end
      timeline.append(run.chat, run.thinking :: number, content.text)
    end
  elseif change.type == "tool_call" then
    tool(run, change.tool_call_id, change)
  elseif change.type == "tool_call_update" then
    tool(run, change.tool_call_id, change)
  elseif change.type == "plan" then
    local entries: { any } = {}
    for _, entry in change.entries do
      table.insert(entries, { content = entry.content, priority = entry.priority, status = entry.status })
    end
    local data = codec.encode { entries = entries }
    if run.plan then
      timeline.set(run.chat, run.plan, { data = data })
    else
      run.speaking, run.thinking = nil, nil
      run.plan = timeline.add(run.chat, "plan", { data = data })
    end
  elseif change.type == "user_message_chunk" then
    -- what the person said: the chat has it already
  elseif change.type == "available_commands_update" or change.type == "current_mode_update" then
    -- the commands a prompt may start with, and the agent's mode: not offered in this app
  elseif change.type == "other" then
    -- an update this app does not know: not shown
  end
end

-- A permission the agent asks for before it goes on: the tool call it is about is updated with what the request
-- says of it, and the question is an item of its own, open until it is answered or the turn ends.
type Asked = {
  id: string,
  tool_call: vnd_agent_client_protocol_ToolCallUpdate,
  options: { vnd_agent_client_protocol_PermissionOption },
}
local function ask(run: Run, event: Asked)
  local call = event.tool_call
  tool(run, call.tool_call_id, call)
  local about = timeline.find(run.chat, "tool", call.tool_call_id, run.since)
  local options: { any } = {}
  for _, option in event.options do
    table.insert(options, { option_id = option.option_id, name = option.name, kind = option.kind })
  end
  run.speaking, run.thinking = nil, nil
  timeline.add(run.chat, "permission", {
    key = event.id,
    text = call.title or (if about then about.name else "The agent asks before it goes on."),
    data = codec.encode { kind = call.kind, options = options },
    done = false,
  })
end

-- why the agent ended its turn, when the person should know: a turn it finished or the person stopped says nothing
local STOPPED: { [string]: string } = {
  max_tokens = "The agent stopped: it reached the most it may write in one answer.",
  max_turn_requests = "The agent stopped: it reached the most requests it may make in one turn.",
  refusal = "The agent refused to go on.",
}

-- how much of the turn's allowance an event takes: all of it, whatever field the agent put its text in
local function weight(event: vnd_agent_client_protocol_TurnEvent): number
  return 200 + #codec.encode(event)
end

-- The agent's turn on `live` with what the person said. It adds to the chat as it goes and returns when the turn is
-- over; a failure is an `error` item in the chat, never an error raised. With `retry`, a connection found closed
-- before the agent did anything is left for the caller to connect again (`lost`), and adds nothing.
function turn.take(chat: string, live: link.Live, text: string, current: Turn, retry: boolean): Outcome
  local handle, refused = live.conn:prompt { session_id = live.session, prompt = { { type = "text", text = text } } }
  if not handle then
    if retry and refused and refused.kind == "closed" then
      return "lost"
    end
    tracing.warn("the agent did not take the prompt", { error = link.sentence(refused), chat = chat })
    timeline.add(chat, "error", { text = link.sentence(refused) })
    return "over"
  end
  current.handle = handle
  local run: Run = { chat = chat, since = timeline.last(chat) }
  local heard = false
  local spent = 0
  while true do
    local event, failed = handle:next()
    if failed then
      -- a turn the person stopped may end with its connection: that is what stopping it is
      if current.stopped then
        return "over"
      end
      if failed.kind == "closed" and retry and not heard then
        return "lost"
      end
      tracing.warn("the agent's turn ended early", { error = link.sentence(failed), kind = failed.kind, chat = chat })
      if failed.kind == "closed" then
        timeline.add(chat, "error", { text = "The connection to the agent closed before it finished answering." })
      else
        timeline.add(chat, "error", { text = link.sentence(failed) })
      end
      return "over"
    end
    if not event then
      return "over"
    end
    heard = true
    spent += weight(event)
    if spent > MAX_TURN then
      tracing.warn("the agent sent more than a turn may hold", { chat = chat, limit = MAX_TURN })
      timeline.add(chat, "error", { text = "The agent sent more than a turn may hold (8 MB), and was stopped." })
      -- the connection goes with what is still coming on it
      link.drop(chat)
      return "over"
    end
    if event.type == "update" then
      update(run, event.update)
    elseif event.type == "permission" then
      ask(run, event)
    elseif event.type == "done" then
      local why = STOPPED[event.stop_reason]
      if why then
        timeline.add(chat, "note", { text = why })
      end
    end
  end
end

return turn
