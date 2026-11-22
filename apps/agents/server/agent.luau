--!strict
-- The agent the app talks to. THIS IS A MOCK: there is no model and no agent process behind it. It answers by a
-- small script (the prompt picks the answer), a piece at a time, as a real agent's answer arrives.
--
-- It is the one seam for the real thing: an agent that is a process the backend starts and speaks ACP with over its
-- stdin and stdout. That needs a neoc module to start a process, which does not exist yet. When it does, this file is
-- what changes: `roster` lists the agents that can be started, and `prompt` sends the turn and calls `said` with each
-- piece of the answer. The rest of the app (main.luau, the client) does not know which of the two it is talking to.
local task = require("std:task")

export type Agent = {
  id: string,
  name: string,
  role: string,
  icon: string,
}

local agent = {}

local ROSTER: { Agent } = {
  {
    id = "scripted",
    name = "Scripted agent",
    role = "Answers by script",
    icon = "bolt",
  },
}

function agent.roster(): { Agent }
  return ROSTER
end

function agent.find(id: string): Agent?
  for _, a in ROSTER do
    if a.id == id then
      return a
    end
  end
  return nil
end

-- what the script says to a prompt, and how long it waits before each piece
local function script(text: string): ({ string }, number)
  local said = string.lower(string.match(text, "^%s*(.-)%s*$") :: string)
  if said == "hello" then
    return { "Hi ", "there" }, 0.05
  elseif said == "slow" then
    local pieces = {}
    for i = 1, 60 do
      table.insert(pieces, `{i} `)
    end
    return pieces, 0.5
  end
  local pieces = { "echo:" }
  for word in string.gmatch(text, "%S+") do
    table.insert(pieces, " " .. word)
  end
  return pieces, 0.03
end

-- One turn: `said` is called with each piece of the answer, in order. It waits between pieces (other calls run
-- meanwhile), and returns when the answer is complete. Cancelling the task that runs it stops the answer.
function agent.prompt(_agent: Agent, text: string, said: (piece: string) -> ())
  local pieces, pause = script(text)
  for _, piece in pieces do
    task.wait(pause)
    said(piece)
  end
end

return agent
