# Agents: key user journeys

The Luau Agents app (`app.luau`) on the host: chats with the agents on its roster. The app only lists agents and chats;
the conversation itself streams over AG-UI (`ChatTranscript`) from the host, which talks to each agent over ACP.

Each journey is a goal, not a script: research participants work out *how* themselves. The automated twin of each is the
test with the same id in `e2e/journeys.spec.ts`, against the real host, in a paired browser, with the scripted ACP agent
(no model; the prompt text picks what it does: `crates/vos-acp/src/bin/fake_agent.rs`). Chats live on the shared host,
so each test starts its own chat and finds it by what it said.

Beyond these journeys: the transcript and session bar rendering is covered by the "Renderer/Chat transcript" stories and
`client/.../kit/agui/*.test.ts`; the roster, thread source, starting and titling a chat by
`crates/vos-host/tests/agents_app.rs`; a restarted host keeping the transcript by the host's own tests (not drivable
on the shared e2e host).

| Id  | Who / goal | Starts at | Done when |
| --- | --- | --- | --- |
| AG1 | Someone opening Agents for the first time: see which agents are on this host. | The desktop with Discover open; Agents is installed but not open. | The Agents window opened from the Dock, listing each agent with its name and role; the desktop's Agents widget lists the same agent. |
| AG2 | Start a chat with an agent and ask it something. | Agents open. | A new empty chat opened; the reply streamed into the transcript after the message; the first message named the chat in the sidebar and later messages did not rename it. |
| AG3 | The agent wants to do something that isn't read-only: decide whether to allow it. | A chat open; the agent asks for permission mid-turn. | The prompt showed in the transcript with the choice to allow or deny; it was still there after reloading the page; allowing it resumed the turn with the agent's answer; once answered, a reload did not bring the prompt back. |
| AG4 | See what the agent reports about its session, and change its mode and model. | A new chat; the agent reports its state after the first message. | The session bar showed the mode before any message; after one it showed the context-usage ring and the slash-command pill, and the mode the agent moved itself to; they changed the mode and the model and the bar followed. |
| AG5 | Stop an agent that is still answering. | A chat open, a slow reply streaming. | They found Stop; the reply stopped, Stop went away and Send came back. |
| AG6 | Close the window and come back to the same conversation. | A chat with a reply in it. | After closing and reopening Agents the same chat was open with its whole transcript. |
| AG7 | Use Agents in a small window: shrink it as far as it goes, then chat. | Agents open. | The window stopped at its minimum (480x420) with nothing cut off; starting a chat opened it, the composer was in view, the reply arrived, and Back returned to the list with the chat in it. |

Node keys (test ids derive from the key path): `new-chat` (or `new-chat-<agentId>` in the menu),
`roster` with `agent-<agentId>`, `sidebar` (the chats), `chat`, `no-chat`. Inside the chat: `<chat id>-thread`,
`-session-mode`, `-session-config-<id>`, `-session-commands`, `-session-usage`, `-composer-input|send|stop`, `-retry`.
