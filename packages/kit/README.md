# @vos/kit: the app kit

The design tokens and web components that every vos app's client uses, so apps built in Svelte, React, Vue, Angular or
plain JavaScript look and behave like one operating environment. The components are custom elements (`<ui-button>`),
authored in-house with [Lit](https://lit.dev) (Decision NEOC-301). **An app never bundles the kit.** The host serves one
copy of it to every app's frame, so a change here reaches every installed app without rebuilding any of them.

| File                        | What it is                                                                                                                                                                                                                                                    |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/kit/tokens.css`           | Every design token as a CSS custom property (colours, type, spacing, radii, shadows, motion) plus the base reset. Generated from the shell's own `packages/ui/src/styles/{tokens,base}.css` (`scripts/kit-tokens.ts`), so a frame and the shell cannot drift. |
| `/kit/kit.js`               | Lit and every component in one classic script (an IIFE: no modules, no external URL, no `eval`, so it loads under the frame's `script-src 'self'`).                                                                                                           |
| `/kit/custom-elements.json` | The Custom Elements Manifest: every tag's attributes, properties, events, slots and CSS parts. The editor types come from it.                                                                                                                                 |

`bun run build:kit` writes the three into `client/public/kit/` (generated, not committed). Vite copies that folder into
`client/dist/kit/`, and the host serves the client's folder, so a frame loads them from the same host.

## Using it from an app

An installed app is its own Vite build output, served by the host as static files, and runs in two ways: **embedded** in a
shell window's iframe, and **standalone** as a full-screen page at `/apps/<id>` (the host's minimal page loads the same
sandboxed app). The vos Vite plugin puts the `<link>` and `<script>` tags for `/kit/tokens.css` and `/kit/kit.js` into the
app's `index.html` at build time; the app never bundles the kit, and the host serves one copy. The kit needs no host to
draw. System settings reach the app's document through the host's bridge in both modes: `data-theme="light|dark"` (and
`color-scheme`) on `<html>`, kept current; the kit also reads `data-reduce-motion` on `<html>`, which zeroes the motion tokens. Until the host has set a
theme (and with none at all, for example in Storybook), the tokens follow the browser's `prefers-color-scheme`, and an
explicit `data-theme` always wins. Components an app uses for its own page (a banner, a list, a field) work the same
everywhere. Sheets, alerts and toasts are not kit components: only the shell draws them, over an embedded app.

The app writes no theme code and imports nothing:

```html
<ui-list label="Tasks">
  <ui-section header="Today">
    <ui-row heading="Buy milk">
      <ui-check slot="leading" label="Buy milk done"></ui-check>
    </ui-row>
  </ui-section>
</ui-list>
<ui-button variant="tinted" hit>Add</ui-button>
```

`hit` is on the button because nothing else gives it a 44 pt touch target (a row does, so the check in it needs none); see
"Touch targets" below.

```css
.card {
  background: var(--bg-grouped-elevated-secondary);
  border-radius: var(--radius-md);
  color: var(--label);
  font: var(--type-body);
}
```

Plain JavaScript, Svelte 5 and React 19 need nothing else: set **properties** (`el.options = [...]`, `<ui-segmented
options={...}>` in React 19 sets the property), listen to **events** (`change`, `input`, `press`, `click`), and put
content in **slots**. Vue needs `compilerOptions.isCustomElement: (tag) => tag.startsWith('ui-')`; Angular needs
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
- **Forms.** `ui-toggle`, `ui-check`, `ui-segmented`, `ui-text-field` and `ui-button` are form-associated and behave in a
  `<form>` as their native counterparts do, whether you set `name`, `value`, `checked`, ... as attributes (plain HTML) or as
  properties (React 19, Svelte 5, Lit): the value is in `FormData`, `disabled` (their own, or a disabled `<fieldset>`'s)
  excludes it, `required` keeps the form invalid until it is met (a check must be on, a segment chosen, a field filled),
  reset and restore work, a `<label for>` names the control, and Enter in a field submits. They have the native API:
  `form`, `labels`, `validity`, `validationMessage`, `willValidate`, `checkValidity()`, `reportValidity()` and
  `setCustomValidity(message)` (the way an app shows its own error). A `ui-button type="submit"` submits the form with its
  own `name` and `value` in the data, but a native submit button is the only thing that can be `event.submitter`: for a
  kit button it is `null`. In an app frame the host's sandbox allows forms, so `submit` fires and Enter and validation work;
  call `preventDefault()` in your handler (a submission nobody prevents sends nothing and leaves the frame on an error page).
- **Events** are the platform's names. `change` (the person committed a value) carries a `detail` on `ui-toggle`,
  `ui-check` (`{ checked }`) and `ui-segmented` (`{ value }`); on `ui-text-field` it is the plain event (read `value`).
  `input` is each keystroke of a field. `press` is a `ui-row` tapped (see below). `click` is the ordinary click: it bubbles
  from everything inside an element, so on a row listen for `press`. The properties are already updated when an event
  fires. Events bubble and cross the shadow boundary.
- **Touch targets (`hit`).** `ui-button`, `ui-check`, `ui-toggle`, `ui-segmented` and `ui-text-field` take the shell's `hit`
  attribute (`packages/ui`'s `hit` prop of `Button`: same name, same default, off). Off, the control is drawn at its own
  size and the element is that size, so the shell's dense layouts (a small button in a card, a grid of buttons 12 px apart)
  are reproduced pixel for pixel. On, the element is a 44 x 44 pt box with the drawn control centred in it (a segment
  grows vertically only). The extra is empty layout space the control owns, so a hit area never covers a neighbour's drawn
  box and a tap inside a drawn box is always that control's. **Set `hit` wherever nothing else gives the control a 44 pt
  target.** A `ui-row` and a `ui-banner` do: the kit controls in them get the box and the row or banner takes the extra
  back from its padding (between two controls, at most half of the gap each), so rows and banners keep the height and the
  text position they have with the shell's controls.
- **Keyboard, focus, motion.** Everything works by keyboard with a visible focus ring, in light and dark, and starts no
  transition under Reduce Motion (busy indicators keep turning).
- **Untrusted text.** Strings an app passes are rendered as text, never as markup.

## Components

### `ui-button`

Attributes: `variant` (`filled` | `tinted` | `gray` | `plain`), `size` (`small` | `medium` | `large` | `fill`), `shape`
(`rounded` | `capsule` | `circle`), `square`, `tint` (CSS colour), `fill` (a data colour, a palette token such as `orange`
or `#rrggbb`: the text colour is picked for 4.5:1), `destructive`, `disabled`, `toggle` and `pressed` (a toggle button:
`aria-pressed`), `type`, `label`, `description`, `name`, `value`, `hit`. Slots: default (the label), `icon`. Part: `button`.
Events: `click`.

### `ui-list`, `ui-section`, `ui-row`

`ui-list`: `variant` (`insetGrouped` | `inset` | `plain` | `grouped` | `sidebar`), `selection` (`none` | `single` |
`multi`, which makes the rows options of a `listbox`), `label`. It owns one tab stop and Up, Down, Home and End.
`ui-section`: `header`, `footer`. `ui-row`: `heading`, `subheading`, `value`, `icon`, `icon-color`, `chevron`,
`tappable` (the row is a button), `selected`, `destructive`, `action`, `disabled`, `label`. Slots: default (custom
content in place of the heading), `leading` (a `ui-check`, say), `trailing` (a `ui-toggle`, say); a control in either is clickable. Event: `press`, fired when the row is tapped
(or, in a list with `selection`, tapped or Enter or Space on the option), not when a control in its slots is: flipping a
trailing toggle does not press the row. Selection is controlled: set `selected`, listen for `press`.

### `ui-toggle`, `ui-check`

A switch and a round check. `checked`, `disabled`, `required`, `label`, `description`, `name`, `value` (what is submitted
while on, `on` by default), `tint`, `hit`. Event: `change` (`detail.checked`).

### `ui-segmented`

`options` (`[{ value, label, disabled? }]`: set the property; a JSON attribute works in plain HTML, but the generated types
accept an array only), `value`, `label`, `block`, `disabled`, `required`, `name`, `hit`. A radio group: arrows, Home and End move
the selection. Event: `change` (`detail.value`).

### `ui-text-field`

`value`, `placeholder`, `type` (`text` | `email` | `password` | `url` | `search` | `tel` | `number`), `label`,
`description`, `name`, `disabled`, `readonly`, `required`, `autocomplete`, `inputmode`, `enterkeyhint`, `autocapitalize`,
`maxlength`, `minlength`, `pattern`, `min`, `max`, `step` (passed to the input inside, which checks them), `invalid`,
`variant` (`field` | `bare`, for inside a `ui-row`), `hit`. Events: `input`, `change` (no `detail`).

### `ui-banner`

An inline message with an action: `tone` (`info` | `success` | `warning` | `error`), `heading`. Slots: default (the
message), `action`. An `error` is announced as an alert, the rest as status.

### `ui-icon`, `ui-spinner`, `ui-empty-state`

`ui-icon`: `name` (the shell's icon set), `size`, `weight`, `label` (decorative without one). `ui-spinner`: `size`,
`label`. `ui-empty-state`: `icon`, `heading`, `message`, slot `action`.

## Adding a component

1. `src/components/ui-<name>.ts`: a `LitElement` (a form control extends `FormControl` in `src/form-control.ts`, which does the form work once), properties declared in `static properties` with a JSDoc line each (the
   manifest reads them), `@tag`, `@slot`, `@fires`, `@csspart` in the class comment, `define('ui-<name>', Class)` from `src/define.ts` (it is safe to load twice), and an entry in
   `src/index.ts`. Start from `base` in `src/styles.ts` (a shadow root does not see the page's `*` rules). Use tokens only
   (`var(--...)`): `scripts/kit-tokens.test.ts` fails on a name that is not a token.
2. Look at the matching control in `packages/ui` and match it: the shell is the visual source of truth.
3. A story file beside it (`ui-<name>.stories.ts`) with every state and a `play` function for the keyboard behaviour. The
   Storybook (`bun run storybook` in this folder) is in the e2e `stories` project: axe, light and dark baselines.
4. Document it above, and `bun run build:kit`.

No `innerHTML`, no `eval`, no external URL (the frame's CSP; `lint:boundaries` and the kit test check it).
