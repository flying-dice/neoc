# Calculator (web)

The Calculator built as a vos app of the new kind (NEOC-298): one Vite project with a contract, a client and a Luau backend.
It is the reference app to copy. The old Calculator (`apps/calculator`, a view-tree app) stays beside it until this one does
everything that one does (its agent tools are next); then this one takes the id `calculator`.

```
openrpc.json        the contract: one method, press(key, state?) -> Screen, and x-app (id, icon, tint, window)
index.html          the page (a root element and the module script)
vite.config.ts      svelte() + vos(), and a stand-in that adds the kit's two tags until the plugin does (NEOC-252)
src/App.svelte      the keypad: the kit's ui-button, the keyboard, the screen
server/main.luau    the handler: press(ctx, args) -> keypad.press(state, key)
server/keypad.luau  the keypad's rules, one key at a time (pure)
server/calc.luau    the arithmetic and the number formats (the old Calculator's, unchanged)
JOURNEYS.md, e2e/   the journeys and their spec
```

**Where the state lives.** The client holds the keypad's state (a plain object it never looks inside) and sends it with each key;
`press` answers with the next state and what to draw. The backend keeps nothing, so each window has a calculator of its own,
and the arithmetic stays the tested Luau of the old Calculator. Keys are sent one after another, so fast typing keeps its order.
Because the state is the page's, **a sum is lost when the window closes or the page reloads** (the old Calculator kept it in
the backend, so it survived both). The shell opens one window per app, so "each window" means today the Calculator's window
in the shell and its own page at `/apps/calculator-web`, which do not share a sum. A state the client sends that cannot be
worked out (a hand-made one) shows "Error" with a plain reason; one that only claims an operator is waiting when none is reads
as the calculator it otherwise is (`+` then makes it `0 +`). `server/calc.luau` is a copy of the old Calculator's; it goes
when this app takes over the id `calculator`.

**Window.** `minW` 320, `minH` 500: below 500 tall the five rows of round keys, the display and the caption no longer fit.

## Build and run it

From a clean checkout, in the repository root (needs Bun, darklua 0.18.0, luau-lsp 1.70.1, StyLua; see `apps/README.md`):

```sh
bun install
bun run build                         # the kit (/kit/kit.js, /kit/tokens.css), then the shell, into client/dist
(cd apps/calculator-web && bun run build)   # the app ref: dist/app-ref.json, app.luau, static/
VOS_INSTALL="file://$PWD/apps/calculator-web/dist/app-ref.json" bun run host
```

Pair the browser with the code the host prints, then open **Calculator (web)** from the home screen, or go to
`http://127.0.0.1:7400/apps/calculator-web` for the app on its own. `bun run build:apps` builds it with every other app into
the registry, so Discover offers it too.

In the project: `bun run lint` (luau-lsp and StyLua), `bun run typecheck` (luau-lsp and svelte-check), `bun run dev`
(rebuild on change; run the host with `VOS_DEV_RELOAD=1`). The tests: `crates/vos-lua/tests/calculator_web.rs` (the keypad
through the generated router, with the old Calculator's tables) and `e2e/journeys.spec.ts`.
