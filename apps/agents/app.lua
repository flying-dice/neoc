-- Agents: chats with the agents on this host. The sidebar lists the agents (from the host's roster) and the
-- chats (SQLite, newest first); the detail is a `ChatTranscript` bound to the selected chat.
--
-- The app keeps only what it needs to list chats: an id, the agent, a title and when it was last used. The
-- conversation itself (messages, tool calls, permission prompts, the agent's settings) lives with the host and
-- streams to the window over AG-UI: the app never sees it, and cannot read an agent's grants or tokens.
local app = require("vos:app")
local ui = require("vos:ui")
local db = app.db

app.manifest {
  id = "agents",
  name = "Agents",
  icon = "sparkles",
  tint = "orange",
  -- 480x420 leaves room for a few messages; the layout itself holds down to 320x280 (one column below 700 wide)
  window = { w = 960, h = 640, minW = 480, minH = 420 },
}

db:migrate {
  [1] = [[
    create table threads (
      id text primary key default (lower(hex(randomblob(8)))),
      agent_id text not null,
      agent_name text not null,
      title text not null default 'New chat',
      updated_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );
    create index threads_by_use on threads(updated_at desc, id desc);
  ]],
}

-- ── Data ────────────────────────────────────────────────────────────────────

local threads = db:query(
  [[select id, title, agent_name, updated_at from threads order by updated_at desc, id desc]],
  { key = "id", page = 60 }
)
local thread = db:row([[select id, title, agent_id, agent_name from threads where id = :id]], { key = "id" })
local chat = app.thread()

local TITLE_MAX = 48

-- the first message names the chat; later messages only move it to the top
local function titleOf(text)
  local line = string.gsub(string.match(text, "^%s*([^\n]*)") or "", "%s+$", "")
  if utf8.len(line) == nil or utf8.len(line) <= TITLE_MAX then
    if line == "" then
      return "New chat"
    end
    return line
  end
  local cut = utf8.offset(line, TITLE_MAX) - 1
  return string.sub(line, 1, cut) .. "…"
end

-- ── Window ──────────────────────────────────────────────────────────────────

app.window("main", {
  title = "Agents",

  client = {
    -- the chat that is open; mirrored to the host so handlers can read it, and kept across restarts
    threadId = { init = "", persist = "app" },
  },

  render = function(w)
    local c = w.client
    local current = thread { id = c.threadId }
    local roster = app.agents:list()
    local chatList = threads {}

    local function startChat(agentId, agentName)
      local made = db:exec("insert into threads(agent_id, agent_name) values (:a, :n)", { a = agentId, n = agentName })
      local row = db:one("select id from threads where rowid = :r", { r = made.last_insert_id })
      if row then
        w.client:set("threadId", row.id)
      end
    end

    local agentRows = {}
    for _, agent in ipairs(roster) do
      table.insert(
        agentRows,
        ui.Row {
          key = "agent-" .. agent.id,
          icon = agent.avatar or "sparkles",
          title = agent.name,
          subtitle = agent.role == "" and "New chat" or agent.role,
          chevron = false,
          onTap = function()
            startChat(agent.id, agent.name)
          end,
        }
      )
    end

    -- one agent: the button starts the chat; several: it opens a menu of them
    local newChat
    if #roster == 1 then
      local only = roster[1]
      newChat = ui.Button {
        key = "new-chat",
        icon = "square.and.pencil",
        label = "New chat",
        shortcut = "cmd+n",
        onTap = function()
          startChat(only.id, only.name)
        end,
      }
    elseif #roster > 1 then
      local items = {}
      for _, agent in ipairs(roster) do
        table.insert(
          items,
          ui.Button {
            key = "new-chat-" .. agent.id,
            title = agent.name,
            icon = agent.avatar or "sparkles",
            onTap = function()
              startChat(agent.id, agent.name)
            end,
          }
        )
      end
      newChat = ui.Menu { key = "new-chat", icon = "square.and.pencil", label = "New chat", unpack(items) }
    end

    return ui.SplitView {
      columns = 2,

      sidebar = ui.Screen {
        title = "Agents",
        toolbar = { trailing = newChat },
        -- two lists: a selectable list may hold only selectable rows, and the agents are actions, not selections
        ui.VStack {
          spacing = 0,
          ui.List {
            key = "roster",
            style = "sidebar",
            label = "Agents",
            empty = ui.EmptyState {
              icon = "sparkles",
              title = "No agents",
              message = "Add an agent to vos.toml, or with agents.create, to start a chat.",
            },
            ui.Section { key = "agents", header = "Agents", unpack(agentRows) },
          },
          -- an empty selectable list is not valid ARIA: show the list only once there is a chat
          ui.Show {
            when = ui.gt(chatList.total, 0),
            ui.List {
              key = "sidebar",
              style = "sidebar",
              label = "Chats",
              selection = c.threadId,
              ui.Section {
                key = "chats",
                header = "Chats",
                ui.ForEach {
                  source = chatList,
                  row = function(r)
                    return ui.Row {
                      tag = r.id,
                      icon = "bubble.left",
                      title = r.title,
                      subtitle = ui.concat(r.agent_name, "  ", ui.fmt.relative(r.updated_at)),
                    }
                  end,
                },
              },
            },
            otherwise = ui.EmptyState {
              key = "no-chats",
              icon = "bubble.left",
              title = "No chats yet",
              message = "Pick an agent above.",
            },
          },
        },
      },

      detail = ui.Screen {
        titleDisplay = "inline",
        title = ui.when(ui.isEmpty(c.threadId), "Agents", current.title),
        ui.Show {
          when = ui.isEmpty(c.threadId),
          ui.EmptyState {
            key = "no-chat",
            icon = "bubble.left.and.bubble.right",
            title = "No chat open",
            message = ui.when(
              ui.isEmpty(current.agent_name),
              "Pick an agent in the sidebar to start a chat.",
              ui.concat("Chat with ", current.agent_name)
            ),
          },
          otherwise = ui.ChatTranscript {
            key = "chat",
            -- both are refs: the transcript follows the selection, and the agent is the one the chat started with
            thread = chat { thread = c.threadId, agent = current.agent_id },
            placeholder = "Message",
            onSend = function(e)
              local id = w.client:get("threadId")
              if type(id) ~= "string" or id == "" then
                return
              end
              db:exec(
                [[update threads
                  set title = case when title = 'New chat' then :t else title end,
                      updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  where id = :id]],
                { t = titleOf(e.payload.text), id = id }
              )
            end,
          },
        },
      },
    }
  end,
})
