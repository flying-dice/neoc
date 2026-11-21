# The Calculator: the reference app

The smallest app that uses every part of an AgentOS app: shared state, a view built from kit components, keyboard
shortcuts, agent tools and a little durable data. Read it next to [`apps/README.md`](../README.md) (the full `app`
API and "Adding an app"); copy the folder to start a new app.

A person taps or types a sum on a keypad; an agent can evaluate an expression, put a result on the keypad, or read the
history. Both use **one calculator** however many windows are open.

## Files

| File | What it is |
|---|---|
| `app.json` | The package manifest Discover shows before the app runs: id, name, `version` (bump it when the source changes), icon, tint, `main`. |
| `app.lua` | The entry: `app.manifest` (window sizes), `app.state`, the key handlers, the window's `render`, the three tools. |
| `calc.lua` | Pure arithmetic with no UI and no `app`: `evaluate` (the expression parser), `combine` (one operation, as the keypad chains), `reason` (the sentence to show for a failure), `format`/`formatEntry` (what the display shows). |
| `JOURNEYS.md` | Seven key user journeys, as goals with an observable "Done when". |
| `e2e/journeys.spec.ts` | Their automated twins (one test per id), against the real host. Not shipped. |
| `../../crates/vos-lua/tests/calculator_app.rs` | The Rust tests of the keypad and the tools, run against the built package. |

It is plain Lua 5.1. `bun run build:apps` bundles `app.lua` and the `calc.lua` it `require`s into one file with
darklua; `app.json` and `app.manifest` must agree on `id`, `icon` and `tint`.

## State: `app.state`

```lua
local s = app.state { entry = "0", value = 0, acc = 0, hasAcc = false, pending = "", fresh = true, caption = "", err = false }
```

One table for the whole app. Reading a field inside `render` subscribes that window to it; assigning a field re-renders
every window that read it. So the key handlers just change `s`, and `render` just reads it. It lives in memory (it is
gone when the host restarts), which suits a keypad; what must survive goes to `app.kv` or `app.db`.

The keypad is a small state machine, in the iPadOS Calculator's way:

- `entry` is the text being typed; once a result is in (`fresh`), the number lives in `value`, so a result is never
  parsed back from its own display text (a result shown as `2e-7` is an ordinary operand).
- Operations chain left to right with no precedence: `2 + 3 × 4` is `(2 + 3) × 4 = 20` (the `calculate` tool does use precedence and
  says 14; which is right for the app is an open owner decision, AGOS-57). Pressing another operator before
  the next number replaces the pending one; `5 + =` doubles it; `=` with nothing pending does nothing.
- `C` clears what is typed, `AC` (nothing typed) clears everything. `±` flips the sign, `%` divides by 100 (it is not a
  percentage of the first operand), Backspace removes the last typed character and leaves a result alone.
- Typing stops at nine digits; a result shows at most nine significant digits, grouped, in exponent form (`1e12`,
  `3.33e-9`) when it will not fit.
- **Errors.** A calculation that cannot be shown (divide by zero, a result that is infinite or not a real number)
  raises a plain sentence in `calc.lua`. `press` catches it: the display says `Error`, the sentence is shown under it
  and announced with `w:announce` (the window's announcer is on the page from the start, so a screen reader hears it;
  the caption is not a live region, because a live region that appears together with its text is often missed). `AC`
  clears it, and so does a digit, which starts a new sum; the other keys have nothing to work on and are ignored. A
  bug in the app is not shown as a source position: `calc.reason` turns anything `calc.lua` did not raise on purpose
  into `Something went wrong`.
- The pending operator is shown three ways: the key is `selected` (an outlined key, and `aria-pressed`), and the caption
  says what is waiting (`27 −`), then the whole sum once `=` is pressed (`27 − 7`).

## View: `app.window` and the kit

`render` is a pure function of `s`: it builds a tree of `@agentos/ui` constructors (`ui.VStack`, `ui.Text`,
`ui.Grid`, `ui.Button`...) and the client draws it with the kit in `packages/ui`. The app picks semantic tokens only
(`style = "display"`, `fill = "gray4"`, `foreground = "secondary"`); it never styles, and it ships no client code. The
components are listed in [`docs/reference/components.md`](../../docs/reference/components.md).

- The keys are one data table (`KEYS`) mapped with `ui.each` into round `ui.Button`s in a four-column `ui.Grid`.
  Every interactive node has a `key`, which becomes its test id (`calculator-key-7`, `calculator-display`).
- The pending operator is `selected` (it is `aria-pressed`, and drawn highlighted).
- The keyboard: each key's `shortcut` prop (`7`, `+`, `*`, `enter`, `escape`, `.`, `%`) is a window-scoped shortcut, and
  two invisible `ui.Shortcut` nodes add `=` and Backspace. Focus has to be inside the window (a window opened from the
  Dock takes it). Enter is `=`, except on a key you walked to with Tab and have not typed on since: Enter and Space then
  press that key, like any button. The client decides from what the person last did (a Tab, a click or touch, or typing
  a character), not from `:focus-visible`, so after tapping keys, or tabbing to a key and typing a sum, Enter is `=`.
- The window is always on a dark canvas (`ui.Scope { scheme = "dark" }`, as on the iPad), in light and dark shell
  themes alike; the keys use palette roles (`gray4`, `gray2`, `orange`) that pick their own text colour for contrast.
- **Minimum window** `320 x 500` (`app.manifest`): the keypad is one fixed-width column (296 pt) centred in the window,
  so it never reflows; below 500 pt tall the keys would drop under 44 pt, so the host stops the window there.

## Tools: what agents can call

Declared with `app.tool`; the host validates the input against the schema before `run`, and publishes them over MCP
as `calculator__<name>`.

| Tool | Does | `readOnly` |
|---|---|---|
| `calculate` | Evaluates an expression with the usual operator precedence (`2 + 3 * 4` is 14), rounded to 12 significant digits (`+ - * / % ^`, parentheses, unary minus, decimals), and returns `result` and its `display` form. Writes the history. | no |
| `show_on_calculator` | Does the same, puts the result on the keypad (the next digit starts a new number; the person can carry on from it with `×`, `+`...) and opens the window. Every open window updates by itself. | no |
| `history` | The last calculations (the newest 50 are kept), newest first, `limit` 1 to 50. | yes |

`calculate` and `show_on_calculator` write the history to `app.kv`, so they are not `readOnly`; `history` only reads, so
it says so, and the host rejects writes from a `readOnly` tool. The history is the one durable thing the app keeps, and
the only thing here beyond the keypad itself: it is there to show `app.kv` and a `readOnly` tool, and a person never
sees it (only what agents calculate is recorded, not what is typed). `show_on_calculator` is the tool that works on the
same `app.state` the window shows: an agent and a person use one calculator. A failing tool returns the same plain sentence the display shows
(`Can't divide by zero`, `Result too large`).

## Run it

```sh
bun run dev        # builds the apps, then the host on :7400 and Vite on :5173; watches apps/** and reloads
```

Open <http://127.0.0.1:5173>, pair with the code the host prints, open Discover and install the Calculator (it is
already installed on the e2e host). Save `app.lua` or `calc.lua` and the running app reloads.

## Check it

```sh
bun run lint:lua       # lua-language-server against the generated LuaCATS API (strict)
bun run lint:stylua    # formatting; `bun run format` fixes it
cargo nextest run -p vos-lua --test calculator_app     # logic, keypad and tools (builds the package first)
bun run test:e2e --project=journeys --grep Calculator      # the seven journeys, headless, against the real host
bun run test:e2e                                           # the whole gate's UI half, with the snapshots
```

The Rust tests tap the real nodes `render` returned (`Keypad::press("1 2 add 7 eq")`), then read the display and caption
back from the next render, so they cover the handlers and the view as well as `calc.lua`. The journeys press the same
keys in a browser, once per journey id. Set `AGENTOS_PORT_BASE=9100` to run the e2e suite in a second worktree.

## Extending it

Add a key by adding a row to `KEYS` and an entry to `actions`; add a tool with one more `app.tool` (a described `input`
schema, `readOnly` when it writes nothing); do arithmetic in `calc.lua` so it stays testable without a window. Then add a
Rust test for it and a line to a journey.
