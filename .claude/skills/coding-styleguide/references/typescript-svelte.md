# TypeScript and Svelte 5 in AgentOS

## TypeScript

- `verbatimModuleSyntax` is on: type-only imports are `import type`.
- Prefer inference for locals and explicit types on exported functions, SDK contracts and props.
- `readonly` on contracts that callers must not mutate; `as const` for constant tables.
- Use `satisfies` to check a literal against a type without widening it.

Expected failure as a tuple, for a new API whose callers must branch:

```ts
type ImportError = { kind: 'too-large'; bytes: number } | { kind: 'unsupported'; type: string }

export function parseImport(file: { size: number; type: string }): [Draft, null] | [null, ImportError] {
  if (file.size > MAX) return [null, { kind: 'too-large', bytes: file.size }]
  if (!ACCEPTED.has(file.type)) return [null, { kind: 'unsupported', type: file.type }]
  return [toDraft(file), null]
}

const [draft, error] = parseImport(file)
if (error !== null) return showError(error) // narrows: draft is Draft below
```

No Result wrapper types or libraries. Async is `Promise<[T, null] | [null, E]>`. Translate only failures you know
(a zod `safeParse` failure, a missing file) and let unknown exceptions escape as bugs. Keep `AbortSignal` and
cleanup paths working.

Parsing at a boundary:

```ts
const Payload = z.object({ expression: z.string().max(500) })
ctx.handle('evaluate', (raw) => {
  const parsed = Payload.safeParse(raw)
  if (!parsed.success) return { ok: false, reason: 'invalid-input' } as const
  return { ok: true, result: evaluate(parsed.data.expression) } as const
})
```

IPC payloads are structured-cloned: plain data only (no functions, class instances or `Error` objects). The memory
host rethrows a handler's real `Error`, but Electron delivers only its message, so don't let tests depend on the
error's type or fields crossing `api.invoke`.

## Svelte 5 (runes only)

| Use | Not |
|---|---|
| `let { label, testid, onpress }: Props = $props()` with an `interface Props` | `export let` |
| `$state`, `$derived` / `$derived.by` | `$:` reactive statements |
| `$effect` only to sync with something outside Svelte (DOM APIs, `api.on` subscriptions, timers) | `$effect` to compute state (use `$derived`) |
| `onclick={…}`, callback props | `on:click`, `createEventDispatcher` |
| snippets: `{#snippet}` / `{@render}`, `children: Snippet` | `<slot>` |
| shared state as a class with `$state` fields in a `*.svelte.ts` module (`lib/settings.svelte.ts`) | `svelte/store` in new code |

- Clean up what you subscribe to: `api.on` returns an unsubscribe; return it from the `$effect`.
- An `async` handler that can fail catches and shows the failure. An unhandled rejection in a component leaves the
  UI in a half-updated state with nothing on screen.
- Agent and tool text is rendered as text, never `{@html}`. The only `{@html}` is `atoms/Icon.svelte`, over the
  constant icon map (`lint:boundaries` allow-list).

## Components

- Header comment `<!-- @<layer> Name — purpose -->` matching the folder (`lint:atomic`).
- Interactive kit components take a required `testid: string` and derive inner ids as `${testid}-<part>`; raw
  interactive elements carry `data-testid` (`lint:testids`, naming in `docs/TESTING.md`).
- Native elements first (`<button>`, `<input>`, `<a>`). A custom control needs a `role`, `tabindex`, key handling
  and an accessible name. Icon-only buttons get `aria-label`.
- Colours, type, spacing, radii and shadows come from tokens (`packages/ui/src/styles/tokens.css`);
  `bun scripts/design-audit.ts <app>` finds raw values. State is never shown by colour alone.
