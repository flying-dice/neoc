# Svelte

Applies to the client, `packages/ui` and every app's page. The TypeScript reference
applies to the script blocks and `.svelte.ts` modules.

## Atomic design

- Five layers, in order: `atoms < molecules < organisms < templates < pages`. Every
  `.svelte` component lives in exactly one layer folder. The only component outside
  them is the composition root (`App.svelte`).
- A component imports components only from strictly lower layers, never its own
  layer or above. Atoms import no components.
- Each component opens with a `<!-- @<layer> Name — what it is -->` header matching
  its folder.
- Reuse before adding: `@vos/ui` through its layer entry points (`@vos/ui/atoms`, …,
  never a deep path) in the client, the kit's `ui-*` elements in an app. Add a new
  atom only when neither has it.
- Pick the layer by what the component knows, not by its size: an atom is one
  element's worth of UI, a molecule is a few atoms with one job, an organism is a
  self-contained section, a template is a layout with slots, a page fills a template
  with real state.
- `bun run lint:atomic` enforces the layers. Make sure it covers the code being
  changed; add the app's glob to `scripts/check-atomic.ts` if it does not.

## State apart from composition

- A component is composition: props in, callbacks out, markup and styles. Atoms
  through templates hold only view-local state (open, hovered, focused, a draft
  input). They never call RPC or the host and never import a module-level store.
- State and behaviour live in `.svelte.ts` modules as rune-based classes or
  functions, with no markup and no DOM. Whatever they need from outside (the `api`
  client, the host, a clock, the window manager) arrives through the constructor as
  a narrow interface, so a test or a story builds one with fakes (see
  `client/src/lib/notifications.svelte.ts`).
- Logic that needs no reactivity is a plain `.ts` module with unit tests next to it.
- The composition root is the only place that constructs state with real
  dependencies. In an app, `App.svelte` imports `api` from its own `@<app id>/rpc` (and `host` from `@vos/host`), builds the
  state and renders one page; the page receives the state (or its values and
  callbacks) as props.
- Prefer `$derived` to an `$effect` that writes state. Keep `$effect` for reaching
  outward (the DOM, a subscription, a timer) and return its cleanup. Use
  `$state.raw` for a value that is replaced whole rather than edited.
- A component that is hard to render without a host or a network has state in it
  that belongs in a `.svelte.ts` module. Move it rather than mocking around it.

## Storybook

- Every app has its own Storybook: `.storybook/main.ts` exporting `defineStorybook()`
  from `@vos/storybook/main`, and a preview built on `@vos/storybook/preview`. An app
  without one is not finished.
- Stories sit next to the component as `Name.stories.svelte` (Svelte CSF,
  `defineMeta`). Every component from atoms to pages has one; a new component and
  its stories arrive in the same change.
- Cover the states a person can meet, one story each: empty, loading, populated,
  failed, disabled, and the longest content the layout must survive. Give each
  interaction a `play` function that asserts on what it changed.
- Feed stories through props and fake dependencies only. No story reaches a running
  host, the network or real time; a fake clock never fires unless the story fires it.
- Render a page story inside `AppWindow` from `@vos/storybook/AppWindow.svelte` at the
  app's manifest minimum size and tag it `app`, so the stories spec measures overflow
  and runs its resize checks.
- Stories are tests: `bun run test:e2e` builds every Storybook and runs the stories
  spec (accessibility, overflow) over each story. Make sure
  `scripts/storybooks.ts` discovers the app's Storybook; add its glob if it does not.

Reference: [Svelte runes](https://svelte.dev/docs/svelte/what-are-runes).
