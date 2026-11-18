# Agents: journeys

The Luau Agents app (`app.luau`) on the host. The conversation itself streams over AG-UI (`ChatTranscript`); the
app only lists agents and chats. Host-level checks live in `crates/agentos-host/tests/agents_app.rs`
(roster in the sidebar, the thread source, starting a chat, titling it); the transcript and session bar are
covered by the "Renderer/Chat transcript" stories and `client/.../kit/agui/*.test.ts`. A Playwright journey spec for
these waits for the e2e harness against the real host (B2).

Node keys: `new-chat` (or `new-chat-<agentId>` in the menu), `agent-<agentId>`, `sidebar`, `chat`. Test ids derive from
the key path; inside the chat they are `<chat id>-thread`, `-session-mode`, `-session-config-<id>`, `-session-commands`,
`-session-usage`, `-composer-input|send|stop` and `-retry`.

## A1. Start a chat

1. Open Agents: the sidebar lists the agents under "Agents" and the chats under "Chats", newest first.
2. With one agent, "New chat" starts a chat with it; with several, it opens a menu of them. Tapping an agent row does
   the same.
3. The chat opens (the agent starts, the session bar shows its mode and model), titled "New chat".

## A2. Talk to an agent

1. Type a message and send: the reply streams, reasoning is collapsed, each turn ends with one line of counts
   ("2 tool calls"), never a list of tools.
2. The first message names the chat in the sidebar; later messages move it to the top.
3. Stop ends a running turn.

## A3. Permissions

1. The agent asks to do something that is not read-only: a prompt appears in the transcript (allow once, always, deny).
2. Answering resumes the turn. A read-only AgentOS tool never prompts under the default policy.
3. After a reload the unanswered prompt is still there.

## A4. Settings

1. Change the mode and the model or effort in the session bar: the agent's session changes and the bar follows.
2. `/` in the composer (or the commands pill) lists the agent's slash commands.
3. The usage ring shows how much of the context window the chat has used.

## A5. Reopen

1. Close the window and reopen it: the open chat comes back (client state `persist = "app"`) with its transcript.
2. Restart the host: the transcript is still there and the next message starts a new agent session.
