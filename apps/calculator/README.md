# Calculator

Arithmetic for a person and their agents, and the reference vos app: one Vite project with a contract, a client and a Luau
backend (NEOC-298). Copy it to start an app.

```
openrpc.json        the contract: press(key, state?) -> Screen, takeShown() -> { screen? }, and x-app (id, icon, tint, window)
index.html          the page (a root element and the module script)
vite.config.ts      svelte() + vos(): the plugin puts the kit's tags into the page and its editor types into .app/
src/App.svelte      the keypad: the kit's ui-button, the keyboard, the screen, an agent's result when it arrives
server/main.luau    the handlers (press, takeShown) and the agent tools (calculate, history, show_on_calculator)
server/keypad.luau  the keypad's rules, one key at a time (pure)
server/calc.luau    the arithmetic and the number formats
JOURNEYS.md, e2e/   the journeys and their spec (CW1 to CW8)
```

**Where the state lives.** The client holds the keypad's state (a plain object it never looks inside) and sends it with each key;
`press` answers with the next state and what to draw. The backend keeps nothing of a sum, so each window has a calculator of its
own. Keys are sent one after another, so fast typing keeps its order. Because the state is the page's, **a sum is lost when the
window closes or the page reloads** (the view-tree Calculator this app replaced kept it in the backend, so it survived both).
The shell opens one window per app, so "each window" means the Calculator's window in the shell and its own page at
`/apps/calculator`, which do not share a sum. A state the client sends that cannot be worked out (a hand-made one) shows "Error"
with a plain reason; one that only claims an operator is waiting when none is reads as the calculator it otherwise is (`+` then
makes it `0 +`).

**What the backend keeps** is what an agent did, in `app.kv`:
- **The history** (`history`: newest first, at most 50, `{ expression, result }`), written by `calculate` and
  `show_on_calculator` and read by `history` (`readOnly`). It is the key and shape the view-tree Calculator used, so a host
  that ran it keeps its history when this one is installed over it.
- **A result waiting to be shown** (`shown`). `show_on_calculator` works the sum out, stores it, tells any open page
  (`app.emit("calculator.shown")`) and asks the shell to open or bring forward the window (`app:openWindow()`). The page calls
  `takeShown` when it starts and on that event: it gets the result once, as a screen whose display is the result and whose
  caption is the agent's expression, and it **replaces the sum the person was in the middle of** (as the view-tree Calculator
  did); the next key carries on from it. With no screen connected the result waits, perhaps a long time, and appears the next
  time the Calculator is opened; a second `show_on_calculator` before then replaces the first. With the window and the
  standalone page both open, whichever asks first shows it.

**Window.** `minW` 320, `minH` 500: below 500 tall the five rows of round keys, the display and the caption no longer fit.

## Build and run it

From a clean checkout, in the repository root (needs Bun, darklua 0.18.0, luau-lsp 1.70.1, StyLua; see `apps/README.md`):

```sh
bun install
bun run build                                 # the kit (/kit/kit.js, /kit/tokens.css), then the shell, into client/dist
(cd apps/calculator && bun run build)         # the app ref: dist/app-ref.json, app.luau, static/
VOS_INSTALL="file://$PWD/apps/calculator/dist/app-ref.json" bun run host
```

Pair the browser with the code the host prints, then open **Calculator** from the home screen, or go to
`http://127.0.0.1:7400/apps/calculator` for the app on its own. `bun run build:apps` builds it with every other app into the
registry, so Discover offers it too. An agent connected over MCP (`<data>/mcp.json`) sees `calculator__calculate`,
`calculator__history` and `calculator__show_on_calculator`.

In the project: `bun run lint` (luau-lsp and StyLua), `bun run typecheck` (luau-lsp and svelte-check), `bun run dev`
(rebuild on change; run the host with `VOS_DEV_RELOAD=1`). The tests: `crates/vos-lua/tests/calculator.rs` (the keypad through
the generated router with the view-tree Calculator's tables, the tools, a waiting result, an earlier history) and
`e2e/journeys.spec.ts`.
