# TypeScript and Svelte 5 in AgentOS (client, kit, scripts)

Applies to `client/` (the shell and the managed renderer), `packages/ui` (the kit), `packages/storybook`, `types/` tests and
`scripts/`. App code is Lua (`lua.md`), the host is Rust (`rust.md`). The client **renders view trees and runs no app
code**: nothing in it knows what the Calculator is.

## TypeScript

- `strict` stays on and `verbatimModuleSyntax` is on: type-only imports are `import type`.
- Prefer inference for locals and explicit types on exported functions and props. `readonly` on contracts callers must not
  mutate; `as const` for constant tables; `satisfies` to check a literal against a type without widening it.
- Untrusted values are `unknown` until parsed or narrowed. No `any`, no `as` on data you didn't construct, no non-null `!`
  (narrow, or state the invariant in a comment where it is established).
- Model alternatives as discriminated unions and close switches with a `never` check.
- Wire types come from the Rust schema: `types/generated/agentos.ts` (`bun run codegen`). Don't hand-write a second copy of
  a wire shape; `client/src/view/protocol.ts` and `shell/hostApi.ts` are tested for drift against the generated
  file. A shape the client reads from outside that the schema doesn't cover (an `/agui` event, a JSON-RPC result) is parsed
  with zod before use.
- Expected failure as a tuple, for a new API whose callers must branch (`Promise<[T, null] | [null, E]>` when async), with an
  error that has a `kind`; throw only for bugs and broken invariants. No `Result` library. Translate only failures you know
  (a zod `safeParse` failure, a closed socket) and let unknown exceptions escape as bugs.

```ts
type ImportError = { kind: 'too-large'; bytes: number } | { kind: 'unsupported'; type: string }

export function parseImport(file: { size: number; type: string }): [Draft, null] | [null, ImportError] {
  if (file.size > MAX) return [null, { kind: 'too-large', bytes: file.size }]
  if (!ACCEPTED.has(file.type)) return [null, { kind: 'unsupported', type: file.type }]
  return [toDraft(file), null]
}
```

- The renderer core (`view/core/*.ts`, `view/rpc/*`) is framework-free plain objects under `bun test`; Svelte is handed a
  `$state` store (`store.svelte.ts`). Keep new logic in the core, where it is testable without a component.
- RPC and AG-UI payloads are JSON: plain data only (no functions, class instances, `Date`s, `Map`s). Notifications can overtake
  responses on `/rpc`; never depend on their order.

## Svelte 5 (runes only)

| Use | Not |
|---|---|
| `let { label, testid, onpress }: Props = $props()` with an `interface Props` | `export let` |
| `$state`, `$derived` / `$derived.by` | `$:` reactive statements |
| `$effect` only to sync with something outside Svelte (DOM APIs, subscriptions, timers) | `$effect` to compute state (use `$derived`) |
| `onclick={...}`, callback props | `on:click`, `createEventDispatcher` |
| snippets: `{#snippet}` / `{@render}`, `children: Snippet` | `<slot>` |
| shared state as a class with `$state` fields in a `*.svelte.ts` module (`lib/host.svelte.ts`) | `svelte/store` in new code |

- Clean up what you subscribe to: return the unsubscribe from the `$effect`.
- An `async` handler that can fail catches and shows the failure. An unhandled rejection leaves the UI half-updated with
  nothing on screen.
- Agent and tool text is rendered as text (or the kit's `Markdown`), never `{@html}` or `innerHTML`. The only `{@html}` is
  `packages/ui/src/atoms/Icon.svelte`, over the constant icon map.

## Renderer adapters (`client/src/view`)

A node `type` maps to an adapter component in the registry (`registry.ts`, `kit.ts`, `kit/*.ts`); the adapter wraps a kit
component. To add one:

- Write it in the right atomic layer folder (`atoms/`, `molecules/`, `organisms/`, ...). It takes `AdapterProps`
  (`node`, `path`, `scope`), reads props through `useView().view.evaluate(expr, scope)` **inside `$derived`** (so a data patch
  repaints exactly the props that read it), renders children with `<Children>`, and passes `commonOf(...).testid` (derived from
  the key path) to the kit component's required `testid`.
- Register it once: `registry.register('Gauge', { component: Gauge, since: '1.2' })`. An unknown type renders its
  `fallback` slot or `Unsupported`.
- A two-way prop is a `Binding` written through `view.bindingOf(...)`: instant and local for `client.*`, a draft overlay and a
  debounced write with `baseRev` for host state. Don't write to the host from a component.
- The component's schema (props, events, slots, accessibility rules) lives in the Rust domain; a new prop or component is a
  schema change first (`bun run codegen`), then the adapter, then a story with a JSON tree.

## Components

- Header comment `<!-- @<layer> Name — purpose -->` matching the folder (`lint:atomic`); layers import only strictly lower
  layers, and `@vos/ui` comes through its layer entry point (`@vos/ui/atoms`), never a deep path.
- Interactive kit components take a required `testid: string` and derive inner ids as `${testid}-<part>`; raw interactive
  elements carry `data-testid` (`lint:testids`, naming in `docs/TESTING.md`).
- Native elements first (`<button>`, `<input>`, `<a>`). A custom control needs a `role`, `tabindex`, key handling and an
  accessible name. Icon-only buttons get `aria-label`.
- Colours, type, spacing, radii and shadows come from tokens (`packages/ui/src/styles/tokens.css`); `bun scripts/design-audit.ts
  <page>` finds raw values in a shell page. State is never shown by colour alone.
- Transitions go through `motion()` (`packages/ui/src/motion.ts`) so reduced motion holds.
