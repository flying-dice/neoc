# @vos/kit: the app kit

The design tokens and web components that every vos app's client uses, so apps built in Svelte, React, Vue, Angular or
plain JavaScript look and behave like one operating environment. The components are custom elements (`<ui-button>`),
authored in-house with [Lit](https://lit.dev) (Decision NEOC-301). **An app never bundles the kit.** The host serves one
copy of it to every app's frame, so a change here reaches every installed app without rebuilding any of them.

| File | What it is |
|---|---|
| `/kit/tokens.css` | Every design token as a CSS custom property (colours, type, spacing, radii, shadows, motion) plus the base reset. Generated from the shell's own `packages/ui/src/styles/{tokens,base}.css` (`scripts/kit-tokens.ts`), so a frame and the shell cannot drift. |
| `/kit/kit.js` | Lit and every component in one classic script (an IIFE: no modules, no external URL, no `eval`, so it loads under the frame's `script-src 'self'`). |
| `/kit/custom-elements.json` | The Custom Elements Manifest: every tag's attributes, properties, events, slots and CSS parts. The editor types come from it. |

`bun run build:kit` writes the three into `client/public/kit/` (generated, not committed). Vite copies that folder into
`client/dist/kit/`, and the host serves the client's folder, so a frame loads them from the same host.

## Using it from an app

An installed app's document runs in two ways: **embedded** in a shell window's iframe, and **standalone** as a full-screen
page of its own (`/apps/<id>`), where the host writes a minimal page around the same sandboxed frame. The kit is loaded by the
app's own document either way and needs no host to draw. Settings arrive on the document the same way in both: the host sets
`data-theme="light|dark"` (and `color-scheme`) on `<html>` and keeps it current; `data-reduce-motion` on `<html>` zeroes the
motion tokens. With no `data-theme` at all (the kit loaded outside any vos page, such as Storybook), the tokens follow the
system setting (`prefers-color-scheme`); an explicit `data-theme` always wins. Components an app uses for its own page (a
banner, a list, a field) work the same everywhere. Sheets, alerts and toasts are not kit components: only the shell draws
them, over an embedded app.

The document the host serves for an app loads the two files, and (embedded) keeps `<html data-theme="light|dark">` and `color-scheme` in step with
the shell. The app writes no theme code and imports nothing:

```html
<ui-list label="Tasks">
  <ui-section header="Today">
    <ui-row heading="Buy milk">
      <ui-check slot="leading" label="Buy milk done"></ui-check>
    </ui-row>
  </ui-section>
</ui-list>
<ui-button variant="tinted">Add</ui-button>
```

```css
.card { background: var(--bg-grouped-elevated-secondary); border-radius: var(--radius-md); color: var(--label); font: var(--type-body); }
```

Plain JavaScript, Svelte 5 and React 19 need nothing else: set **properties** (`el.options = [...]`, `<ui-segmented
options={...}>` in React 19 sets the property), listen to **events** (`change`, `input`, `click`), and put content in
**slots**. Vue needs `compilerOptions.isCustomElement: (tag) => tag.startsWith('ui-')`; Angular needs
`CUSTOM_ELEMENTS_SCHEMA` on the module or standalone component.

### Editor types

The build writes the Custom Elements Manifest (`/kit/custom-elements.json`), and `scripts/kit-types.ts` turns it into a
declaration file per framework that augments the framework's own typing, so a wrong attribute, a wrong value
(`variant="tinteed"`) or a wrong event payload is flagged in the editor and the build:

- **Svelte** (`svelte-check`): `SvelteHTMLElements` gets every `ui-*` tag. Handlers are `onchange`, `oninput`.
- **React 19** (`tsc`): `JSX.IntrinsicElements` gets every `ui-*` tag. Listen with the **lower-case** `onchange` and
  `oninput`: React's `onChange` is its own synthetic event, which a custom element never raises, so the types leave it
  out and it is an error rather than a handler that silently never runs. `onClick` is fine.

`packages/kit/fixtures/` holds a React and a Svelte file that must type-check and a Svelte file whose every element must be
an error; `scripts/kit-types.test.ts` runs them. (Svelte's accessibility lint warns that a `ui-*` element with a click
handler has no role; the element handles its own keys, so silence that warning for `ui-` tags.)

### Rules every component follows

- **Test id.** Put `data-testid` on the element (`<ui-button data-testid="save">`). Parts that are interactive on their
  own carry derived ids: a segment of `ui-segmented` is `<testid>-<value>`.
- **Name.** An interactive control has an accessible name: its visible text, or `label`. A `<label for>` that points at a
  `ui-toggle`, `ui-check` or `ui-text-field` names it too. `description` is read after the name.
- **Forms.** `ui-toggle`, `ui-check`, `ui-segmented`, `ui-text-field` and `ui-button type="submit"|"reset"` take part in a
  `<form>`: `name`, `value`, `required`, reset, a disabled fieldset, Enter to submit.
- **Events** are the platform's names: `change` (the person committed a value; `detail` carries it, and the property is
  already updated), `input` (each keystroke), `click`. They bubble and cross the shadow boundary.
- **Keyboard, focus, motion.** Everything works by keyboard with a visible focus ring, in light and dark, and starts no
  transition under Reduce Motion (busy indicators keep turning).
- **Untrusted text.** Strings an app passes are rendered as text, never as markup.

## Components

### `ui-button`
Attributes: `variant` (`filled` | `tinted` | `gray` | `plain`), `size` (`small` | `medium` | `large` | `fill`), `shape`
(`rounded` | `capsule` | `circle`), `square`, `tint` (CSS colour), `fill` (a data colour, a palette token such as `orange`
or `#rrggbb`: the text colour is picked for 4.5:1), `destructive`, `disabled`, `toggle` and `pressed` (a toggle button:
`aria-pressed`), `type`, `label`, `description`, `name`, `value`. Slots: default (the label), `icon`. Part: `button`.
Events: `click`.

### `ui-list`, `ui-section`, `ui-row`
`ui-list`: `variant` (`insetGrouped` | `inset` | `plain` | `grouped` | `sidebar`), `selection` (`none` | `single` |
`multi`, which makes the rows options of a `listbox`), `label`. It owns one tab stop and Up, Down, Home and End.
`ui-section`: `header`, `footer`. `ui-row`: `heading`, `subheading`, `value`, `icon`, `icon-color`, `chevron`,
`tappable` (the row is a button), `selected`, `destructive`, `action`, `disabled`, `label`. Slots: default (custom
content in place of the heading), `leading`, `trailing` (a `ui-toggle`, say). Selection is controlled: set `selected`,
listen for `click` on the row.

### `ui-toggle`, `ui-check`
A switch and a round check. `checked`, `disabled`, `label`, `description`, `name`, `value`, `tint`. Event: `change`
(`detail.checked`).

### `ui-segmented`
`options` (`[{ value, label, disabled? }]`, a property), `value`, `label`, `block`, `disabled`, `name`. A radio group:
arrows, Home and End move the selection. Event: `change` (`detail.value`).

### `ui-text-field`
`value`, `placeholder`, `type` (`text` | `email` | `password` | `url` | `search` | `tel`), `label`, `description`, `name`,
`disabled`, `readonly`, `required`, `autocomplete`, `invalid`, `variant` (`field` | `bare`, for inside a `ui-row`).
Events: `input`, `change`.

### `ui-banner`
An inline message with an action: `tone` (`info` | `success` | `warning` | `error`), `heading`. Slots: default (the
message), `action`. An `error` is announced as an alert, the rest as status.

### `ui-icon`, `ui-spinner`, `ui-empty-state`
`ui-icon`: `name` (the shell's icon set), `size`, `weight`, `label` (decorative without one). `ui-spinner`: `size`,
`label`. `ui-empty-state`: `icon`, `heading`, `message`, slot `action`.

## Adding a component

1. `src/components/ui-<name>.ts`: a `LitElement`, properties declared in `static properties` with a JSDoc line each (the
   manifest reads them), `@tag`, `@slot`, `@fires`, `@csspart` in the class comment, `customElements.define`, and an entry in
   `src/index.ts`. Start from `base` in `src/styles.ts` (a shadow root does not see the page's `*` rules). Use tokens only
   (`var(--...)`): `scripts/kit-tokens.test.ts` fails on a name that is not a token.
2. Look at the matching control in `packages/ui` and match it: the shell is the visual source of truth.
3. A story file beside it (`ui-<name>.stories.ts`) with every state and a `play` function for the keyboard behaviour. The
   Storybook (`bun run storybook` in this folder) is in the e2e `stories` project: axe, light and dark baselines.
4. Document it above, and `bun run build:kit`.

No `innerHTML`, no `eval`, no external URL (the frame's CSP; `lint:boundaries` and the kit test check it).
