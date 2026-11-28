# Color

vos colours are iOS's dynamic system colours, declared once in `tokens.css` with `light-dark()`, so every token is
right in both themes. An app never picks a colour value: it picks a token by meaning (a label, a background, a
separator, a fill, the tint), and the token gives the value for the theme. The repo departs from Apple only to reach
WCAG AA in the default appearance, because vos has no Increase Contrast setting to fall back on:

- `--label-secondary` is 0.75 alpha in light mode (Apple: 0.6), for 4.8:1 on grouped backgrounds.
- `--ios-blue`, `--ios-red`, `--tint` (blue by default) and `--focus-ring` are Apple's Increase Contrast variants.
- `--tint-fill` darkens the tint in dark mode so white text on it keeps 4.5:1.

Where Apple's value and these differ, the token wins. One departure is not about contrast: the accent is the
person's, chosen in Settings as on macOS, not each app's own as on iPadOS (COLOR-07).

## Tokens by meaning

| Meaning | Tokens |
| --- | --- |
| Text | `--label`, `--label-secondary`, `--label-tertiary` (disabled text, glyphs only), `--label-quaternary` (watermarks) |
| Acts on tap | `--tint`, `--link`; white on `--tint-fill` |
| Destructive, error | `--ios-red` |
| Plain backgrounds | `--bg`, `--bg-secondary`, `--bg-tertiary`; on windows and sheets `--bg-elevated` |
| Grouped backgrounds | `--bg-grouped`, `--bg-grouped-secondary`, `--bg-grouped-tertiary`; on windows and sheets `--bg-grouped-elevated`, `--bg-grouped-elevated-secondary` |
| Lines | `--separator` (translucent hairline), `--separator-opaque` |
| Small shapes (fields, tracks, thumbs' wells) | `--fill`, `--fill-secondary`, `--fill-tertiary`, `--fill-quaternary` |
| Overlays | `--material-*`, `--dimming` |
| Data, icon tiles, charts | the twelve `--ios-*` system colours and the grays |
| Fixed purpose | `--focus-ring`, `--window-grip`, `--switch-off`, `--segment-thumb`, `--knob` |

---

## Tokens only

### COLOR-01 No colour literals in app code
- **Rule:** App CSS, inline styles and component styles never contain a colour literal (`#hex`, `rgb()`, `hsl()`,
  `oklch()`, a named colour other than `transparent` and `currentColor`); colours are `var(--token)` or a
  `color-mix()` of tokens done by the kit.
- **Why (iPadOS):** Color; Dark Mode.
- **Check:** `source`: in `apps/**` and `client/src/**` (not `tokens.css`), a match of
  `/#[0-9a-f]{3,8}\b|\b(rgb|rgba|hsl|hsla|oklch|oklab|lab|lch)\(|\b(white|black|gray|grey|red|blue|green)\b/i` inside
  a style value.
- **Wrong / Right:** `color: #8e8e93` / `color: var(--label-secondary)`

### COLOR-02 Every colour on screen is a token value
- **Rule:** Every colour an app's own elements compute (text, background, border, outline, SVG fill and stroke) is
  the value of a colour token in the active theme, or `transparent`.
- **Why (iPadOS):** Color.
- **Check:** `auto`: on elements outside kit and shell shadow roots, a computed colour property not within 2/255
  per channel and 0.02 alpha of any colour token resolved for the theme (auto-checks.md lists the tokens).
- **Wrong / Right:** `background: #f5f5f7` / `background: var(--bg-grouped-elevated)`

### COLOR-03 Text colours come from the label set
- **Rule:** Text is coloured with `--label`, `--label-secondary`, `--tint`/`--link`, `--ios-red`, or white on a
  filled tint or system colour. Other system colours do not colour text.
- **Why (iPadOS):** Color; Labels.
- **Check:** `auto`: a text node's computed `color` that is not one of those tokens (or white over a tint or system
  colour fill), `--label-tertiary` aside (see TYPE-43).
- **Wrong / Right:** `color: var(--ios-green)` for "Running" / `--label` text beside a green status glyph.

### COLOR-04 Semantic tokens keep their meaning
- **Rule:** A token is used only for its role: no label colour as a background, no separator or fill as text, no
  background token as text or border.
- **Why (iPadOS):** Color.
- **Check:** `auto`: computed `color` equal to a `--bg*`, `--fill*` or `--separator*` token; computed
  `background-color` equal to a `--label*` token on an element larger than 24×24px.
- **Wrong / Right:** `background: var(--label-secondary)` for a panel / `background: var(--bg-grouped-elevated-secondary)`

### COLOR-05 Red and blue are the accessible variants
- **Rule:** Use `--ios-red` and `--ios-blue` as defined; never Apple's default red or blue values, which miss 4.5:1.
- **Why (iPadOS):** Color; Accessibility. The repo departs from Apple's defaults on purpose.
- **Check:** `auto`: a computed colour equal to `#ff3b30`, `#ff453a`, `#007aff` or `#0a84ff`.
- **Wrong / Right:** `color: #007aff` / `color: var(--tint)`

## Tint and meaning

### COLOR-06 Tint means it acts
- **Rule:** `--tint` colours only what a person can act on: buttons, links, bar items, a selected control, the
  checkmark of a chosen option. Static text, headings and decoration are never tinted.
- **Why (iPadOS):** Color; Buttons.
- **Check:** `auto`: an element whose computed `color` or `background-color` equals `--tint` and that is not, and is
  not inside, an interactive element (`a[href]`, `button`, `[role=button|link|tab|menuitem|option|switch|checkbox|radio]`,
  `input`, `select`, `ui-button`, `ui-row[tappable]`, `ui-row[action]`, `ui-segmented`, `ui-toggle`, `ui-check`) or a
  `ui-meter` fill.
- **Wrong / Right:** a blue section title / a blue `Edit` button.

### COLOR-07 The tint is the person's accent
- **Rule:** An app does not set its own `--tint`, nor `tint` or `--toggle-tint` on a kit element (a switch stays
  green); it uses the accent chosen in Settings. A one-off colour is a data colour (`ui-button fill`), not an accent.
- **Why (iPadOS):** Color, App accent colors (macOS) and Sidebars: on macOS the accent people choose replaces an app's
  own. vos departs from iPadOS, where each app sets its accent, so every app tints alike. A contrast finding on
  tinted text under an accent other than blue is the token's (the Settings choices), not the app's.
- **Check:** `source`: an app declaring `--tint` or `--toggle-tint`, or setting `tint=` on a kit element.
- **Wrong / Right:** `:host { --tint: var(--ios-orange) }` / nothing; `<ui-button fill="orange">` for a colour-coded
  label.

### COLOR-08 Red is for destroying and for errors
- **Rule:** `--ios-red` marks a destructive action or an error and nothing else: not emphasis, not a brand, not
  "important".
- **Why (iPadOS):** Color; Buttons.
- **Check:** `auto`: an element in `--ios-red` that is not, and is not inside, `[destructive]`, `.destructive`,
  `[footer-tone=error]`, `[invalid]`, `[aria-invalid=true]`, `[role=alert]` or `ui-banner[tone=error]`; or an icon
  tile or data mark (allowed).
- **Wrong / Right:** a red "New" label / a red `Delete` menu item.

### COLOR-09 What destroys is red
- **Rule:** A text button, menu item, swipe action or row whose action deletes or cannot be undone is red, set with
  the element's `destructive` property (`ui-button destructive`, `ui-row destructive`, `ui-alert destructive`, a
  `destructive` menu item), not with a colour.
- **Why (iPadOS):** Buttons; Menus; Alerts.
- **Check:** `auto`: a `menuitem`, alert button, swipe action or `ui-row[tappable]` whose name matches
  `/^(Delete|Erase|Remove|Discard|Empty Trash|Reset)\b/` and whose text is not `--ios-red`; `source` for a hand-set red.
- **Wrong / Right:** `<ui-row heading="Delete Volume" tappable style="color:var(--ios-red)">` /
  `<ui-row heading="Delete Volume" tappable destructive>`

## Backgrounds, separators, fills

### COLOR-10 Grouped lists sit on a grouped background
- **Rule:** An `insetGrouped` or `grouped` list sits directly on `--bg-grouped-elevated` (a window, a sheet) or
  `--bg-grouped` (the desktop), so its rows (`--bg-grouped-elevated-secondary`) stand out; a `plain` or `inset` list
  sits on the plain background.
- **Why (iPadOS):** Color.
- **Check:** `auto`: the nearest ancestor with an opaque background of a `ui-list[variant=insetGrouped|grouped]` is not
  `--bg-grouped` or `--bg-grouped-elevated`.
- **Wrong / Right:** an inset-grouped list on `--bg-elevated` (white rows on white) / on `--bg-grouped-elevated`.

### COLOR-11 Surfaces above the desktop are elevated
- **Rule:** A window, sheet, popover or alert uses the elevated backgrounds (`--bg-elevated`, `--bg-grouped-elevated`),
  never `--bg` or `--bg-grouped`, which are black in dark mode and lose the layering.
- **Why (iPadOS):** Dark Mode.
- **Check:** `auto`: in the dark theme, a `[role=dialog]`, `[role=alertdialog]`, popover or window content root whose
  computed background is `#000000`, except an always-dark media or terminal canvas (COLOR-18).
- **Wrong / Right:** `.sheet { background: var(--bg) }` / `.sheet { background: var(--bg-elevated) }`

### COLOR-12 Lines are separator hairlines
- **Rule:** Dividers between rows, columns and bars are `--separator` (or `--separator-opaque` over moving content)
  at `--hairline` or 1px; never a gray of your own or a thicker rule.
- **Why (iPadOS):** Color; Lists and tables.
- **Check:** `auto`: an app element's visible border (width ≤ 2px) or 1px divider element whose colour is not
  `--separator`, `--separator-opaque`, `--ios-red` (invalid) or `--focus-ring`.
- **Wrong / Right:** `border-bottom: 1px solid #ddd` / `box-shadow: 0 var(--hairline) 0 var(--separator)`

### COLOR-13 Fills are for small shapes
- **Rule:** `--fill*` tokens fill small controls (a field, a track, a segmented well, a pressed row); they are never a
  view or card background.
- **Why (iPadOS):** Color.
- **Check:** `auto`: an element of more than 40,000px² whose background is a `--fill*` token.
- **Wrong / Right:** `.panel { background: var(--fill-tertiary) }` / `.panel { background: var(--bg-grouped-elevated-secondary) }`

## Contrast

### COLOR-14 Text holds 4.5:1
- **Rule:** Text is at least 4.5:1 against what is actually behind it (composited through translucent layers), in
  both themes, in the default appearance; disabled text is exempt.
- **Why (iPadOS):** Accessibility; Dark Mode.
- **Check:** `auto`: WCAG contrast of computed `color` over the composited background below 4.5:1, unless the text is
  large (COLOR-15) or inside a disabled control.
- **Wrong / Right:** `--label-tertiary` footnote on white (1.9:1) / `--label-secondary` (4.8:1).

### COLOR-15 Large text holds 3:1
- **Rule:** Text of 24px or more, or 18.66px or more at weight 700, is at least 3:1. The repo uses WCAG's sizes,
  which are stricter than the 18 pt Apple's inspector uses.
- **Why (iPadOS):** Accessibility.
- **Check:** `auto`: large text (as defined) under 3:1.
- **Wrong / Right:** a 28px title in `--ios-orange` on white (2.0:1) / in `--label`.

### COLOR-16 Glyphs and control edges hold 3:1
- **Rule:** An icon that carries meaning, a focus ring, and any part needed to see a control's state hold 3:1 against
  their neighbours.
- **Why (iPadOS):** Accessibility; Color.
- **Check:** `auto`: the computed `color`/`stroke`/`fill` of a meaningful `ui-icon` or `svg` (one with an accessible
  name, or the only content of a control) under 3:1 against the composited background.
- **Wrong / Right:** a `--label-quaternary` gear as the only content of a button / `--tint`.

### COLOR-17 Placeholders hold 4.5:1
- **Rule:** Placeholder text is text and holds 4.5:1, as `SearchField` does with `--label-secondary`. Apple's
  tertiary placeholder (the current `--placeholder` token) does not; that token is the known gap, not something an
  app patches locally.
- **Why (iPadOS):** Accessibility; Text fields. The repo departs from Apple for contrast.
- **Check:** `auto`: `getComputedStyle(input, '::placeholder').color` over the field's background under 4.5:1.
- **Wrong / Right:** placeholder in `--label-tertiary` / in `--label-secondary`.

## Signals and themes

### COLOR-18 Colour is never the only signal
- **Rule:** Every state shown by colour (error, success, running, stopped, selected, unread) is also shown by text, a
  glyph's shape or position.
- **Why (iPadOS):** Color; Accessibility.
- **Check:** `auto`: an element of 16×16px or less with a system-colour background, no text, no accessible name, and no
  sibling text; `look` for red/green pairs.
- **Wrong / Right:** a green or red dot alone / the dot plus the word `Running`.

### COLOR-19 Both themes, from one declaration
- **Rule:** Every screen works in light and dark from the tokens alone: no `prefers-color-scheme` queries or
  `[data-theme]` branches in app code.
- **Why (iPadOS):** Dark Mode.
- **Check:** `source`: `prefers-color-scheme` or `data-theme` in `apps/**`. (Every computed check also runs in both
  themes: see auto-checks.md, Runs.)
- **Wrong / Right:** `@media (prefers-color-scheme: dark) { .x { color: #eee } }` / `.x { color: var(--label) }`

### COLOR-20 An app never forces a theme on its chrome
- **Rule:** An app's bars, lists and controls follow the theme the person chose in Settings; only a content canvas
  that shows media or a terminal may be always dark. (Offering a theme switch of its own is HELP-13.)
- **Why (iPadOS):** Dark Mode.
- **Check:** `auto`: an element inside an app's window with a `data-theme` attribute, or `color-scheme` fixed to one
  value, that contains a bar, a `ui-list` or a `ui-button`.
- **Wrong / Right:** `<main data-theme="dark">` around the whole app / `data-theme="dark"` on the terminal canvas only.

### COLOR-21 Nothing relies on Increase Contrast
- **Rule:** vos has no Increase Contrast setting, so the default colours meet the contrast rules on their own; an
  app does not add a high-contrast variant of its own.
- **Why (iPadOS):** Accessibility; Color.
- **Check:** `source`.
- **Wrong / Right:** `@media (prefers-contrast: more)` in one app / tokens that already pass.
