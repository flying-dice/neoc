# Agents: key user journeys

The Agents app: chats with the agents on this host's roster. Its page (Svelte, on the kit's tokens) runs in a frame of its
window; its backend (`server/main.luau`) keeps only the list of chats and which one is open. The conversation itself is the
host's: the page reaches it through the host page's agent chat services (`vos/agui/*`), which make the AG-UI requests to
`/agui` with the host page's own session, and the host talks to each agent over ACP.

Each journey is a goal, not a script: research participants work out *how* themselves. The automated twin of each is the
test with the same id in `e2e/journeys.spec.ts`, against the real host, in a paired browser, through the app's frame, with
the scripted ACP agent (no model; the prompt text picks what it does: `crates/vos-acp/src/bin/fake_agent.rs`). Chats live on
the shared host, so each test starts its own chat and finds it by what it said.

Beyond these journeys: the chat's client logic (folding AG-UI events into a transcript, permission prompts, stop, settings,
pulling a run's events) by `src/agui/*.test.ts`; the host page's services (whose thread, the bounds, teardown, errors) by
`client/src/shell/hostServices/agui.test.ts`; the backend's handlers by `crates/vos-lua/tests/agents_app.rs`; AG8 by the host's own test
(`crates/vos-host/tests/agents_upgrade.rs`), not a Playwright spec, because the shared e2e host cannot start with the old Agents
app installed.

| Id  | Who / goal | Starts at | Done when |
| --- | --- | --- | --- |
| AG1 | Someone opening Agents for the first time: see which agents are on this host. | The desktop; Agents is installed but not open. | The Agents window opened from the Dock, listing each agent with its name and role; the desktop's Agents widget lists the same agent. |
| AG2 | Start a chat with an agent and ask it something. | Agents open. | A new empty chat opened; the reply streamed into the transcript after the message; the first message named the chat in the sidebar and later messages did not rename it. |
| AG3 | The agent wants to do something that isn't read-only: decide whether to allow it. | A chat open; the agent asks for permission mid-turn. | The prompt showed in the transcript with the choice to allow or deny; it was still there after reloading the page; allowing it resumed the turn with the agent's answer; once answered, a reload did not bring the prompt back. |
| AG4 | See what the agent reports about its session, and change its mode and model. | A new chat; the agent reports its state after the first message. | The session bar showed the mode before any message; after one it showed the context-usage ring and the slash-command pill, and the mode the agent moved itself to; they changed the mode and the model and the bar followed. |
| AG5 | Stop an agent that is still answering. | A chat open, a slow reply streaming. | They found Stop; the reply stopped, Stop went away and Send came back. |
| AG6 | Close the window and come back to the same conversation. | A chat with a reply in it. | After closing and reopening Agents the same chat was open with its whole transcript. |
| AG7 | Use Agents in a small window: shrink it as far as it goes, then chat. | Agents open. | The window stopped at its minimum (480x420) with nothing cut off; starting a chat opened it, the composer was in view, the reply arrived, and Back returned to the list with the chat in it. |
| AG8 | Someone who chatted before Agents was rebuilt as an app with a page of its own finds their chats. | A host whose Agents app was the view-tree one, with a chat in it, upgraded to this one. | The chat is listed with its title, opens with its transcript, and a new message keeps its name. |

Test ids (in the frame): `agents-app` (its `aria-busy` is false once the list has loaded), `agents-roster` with
`agents-agent-<agentId>`, `agents-chats` with `agents-chat-<chatId>`, `agents-new-chat` (`agents-new-chat-<agentId>` in its
menu when there are several agents), `agents-back` (one column), `agents-no-agents`, `agents-no-chats`, `agents-no-chat`,
`agents-error` (+ `-retry`). The open chat is `agents-chat`: `-thread` (with `-user-<id>`, `-reply-<id>`,
`-permission-<id>` and its options, `-error-<id>`), `-session-mode`, `-session-config-<id>`, `-session-commands`,
`-session-usage`, `-composer-input|send|stop|attach`, `-retry`.
