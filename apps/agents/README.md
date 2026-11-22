# Agents

Chat with the agents on this host. A vos app of the new kind (copied from `apps/calculator-web`): one Vite project with a
contract, a client and a Luau backend.

```
openrpc.json         the contract: agents, chats, startChat, openChat, noteMessage; x-app (id agents, icon, window)
index.html           the page
vite.config.ts       svelte() + vos(), and the stand-in that adds the kit's two tags until the plugin does (NEOC-298)
src/App.svelte       the sidebar (the roster, the chats) and the open chat; one column below 700 px wide
src/Chat.svelte      one chat: transcript, session bar, composer
src/chat/            the chat's components (moved from packages/ui): ChatThread, Composer, SessionBar, PermissionCard, ...
src/agui/            the chat's client logic (moved from the shell): ChatSession folds AG-UI events into a transcript;
                     transport.ts is the host page's vos/agui/* services; fakeAgui.ts is a scripted /agui for the tests
server/main.luau     the handlers: the chat list in the app's database (the view-tree app's table and migration, kept)
server/title.luau    a chat's title from its first message
JOURNEYS.md, e2e/    the journeys and their spec
```

**Where things live.** The backend keeps only what the list needs: a chat's id, its agent, a title and when it was last
used, and which chat was open (`app.kv`), so the window reopens on it. The conversation (messages, tool calls, permission
prompts, the agent's session settings) is the host's AG-UI thread `agents:<chat id>`. The page cannot reach `/agui` itself
(its frame has no cookie and may not `fetch`), so it asks the host page, with `host.call('vos/agui/...')`; the host page
makes the request with its own session and puts the frame's app id in front of the chat id, so a page only ever reaches its
own app's chats. A run's events are pulled (`vos/agui/events`, naming the last event the page has), which keeps a fast agent
from flooding a slow page. Closing the window mid-answer cancels the turn, unless it is waiting on a permission prompt,
which stays for when the chat is opened again (as the view-tree app's window did). `client/src/shell/hostServices/agui.ts`
documents the services.

**Untrusted text.** What an agent or a tool says is shown as text: `RichText` builds nodes for its small Markdown subset,
and nothing here uses `{@html}` or `innerHTML` (`bun run lint:boundaries` checks an app's `src/` too).

**Window.** `minW` 480, `minH` 420: room for the composer, the session bar and a few messages in one column.

## Build and run it

From the repository root (needs Bun, darklua 0.18.0, luau-lsp 1.70.1, StyLua; see `apps/README.md`):

```sh
bun install
bun run build                         # the kit and the shell, into client/dist
(cd apps/agents && bun run build)     # the app ref: dist/app-ref.json, app.luau, static/
VOS_INSTALL="file://$PWD/apps/agents/dist/app-ref.json" bun run host
```

Add an agent to the host's config (`[[agents]]` in `vos.toml`) to have someone to chat with. In the project: `bun run test`
(the chat's logic), `bun run lint`, `bun run typecheck`. The backend's tests are `crates/vos-lua/tests/agents_app.rs`; the
upgrade from the view-tree app is `crates/vos-host/tests/agents_upgrade.rs`.
