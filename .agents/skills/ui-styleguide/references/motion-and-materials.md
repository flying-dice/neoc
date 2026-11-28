# Motion and materials

Materials are the blurred, translucent surfaces that float above content: bars, menus, alerts, the Dock. Motion is
how things move between states. Both come from tokens: `--material-*` with `--material-blur` (the `.ios-material-*`
classes, the shell's `Material` atom), and `--ease-ios`, `--ease-spring`, `--duration-fast`, `--duration`,
`--duration-slow`. Under Reduce Transparency the material tokens turn opaque; under Reduce Motion the base reset stops
every transition (A11Y-15; MOTION-03 checks the materials).

## Materials per surface

| Surface | Material | Where it is done |
| --- | --- | --- |
| Navigation bar, toolbar (once content scrolls under it) | `--material-chrome`, with a hairline | `NavigationBar` |
| Alert, context menu, multitasking menu, popover, action sheet, Spotlight panel | `--material-thick` | `Alert`, `ContextMenu`, `MultitaskingMenu`, `Spotlight` |
| Dock | `--material-thin` | `Dock` |
| Desktop widget, platter | `--material-regular` | `Card` |
| Window, sheet, list, section, field | none: opaque backgrounds | `Window`, `ui-list` |
| Behind a modal | `--dimming` (no blur) | `Scrim`, `dialog::backdrop` |

## Motion tokens

| Token | Value | For |
| --- | --- | --- |
| `--duration-fast` | 180 ms | a press, a hover, a colour or opacity change, a small control (a segment thumb's fade) |
| `--duration` | 300 ms | something that moves: a switch knob, a segment thumb, a row inserted, a view pushed |
| `--duration-slow` | 450 ms | a large surface: a window changing layout, a sheet, Slide Over |
| `--ease-ios` | `cubic-bezier(0.32, 0.72, 0, 1)` | every movement |
| `--ease-spring` | `cubic-bezier(0.34, 1.36, 0.64, 1)` | a small pop that should overshoot (an icon landing), sparingly |

---

## Materials

### MOTION-01 Blur is for what floats
- **Rule:** Blur and translucency appear only on surfaces that float above content (bars, menus, popovers, alerts,
  the Dock, Spotlight, desktop widgets); content inside a window (lists, sections, cards, panes) is opaque.
- **Why (iPadOS):** Materials.
- **Check:** `auto`: an element inside a window's content root, other than a bar, menu, popover or dialog, with a
  computed `backdrop-filter` other than `none`.
- **Wrong / Right:** a frosted card in a list / an opaque `ui-section`.

### MOTION-02 Each surface has its material
- **Rule:** A floating surface uses the material of its kind (table above): chrome for bars, thick for alerts, menus and
  popovers, thin for the Dock, regular for widgets.
- **Why (iPadOS):** Materials.
- **Check:** `auto`: a scrolled navigation bar whose background is not `--material-chrome`; an open alert, menu or
  popover whose surface background is not `--material-thick`.
- **Wrong / Right:** a context menu on `--material-ultrathin` / on `--material-thick`.

### MOTION-03 Materials come from the tokens
- **Rule:** A translucent surface is made with `.ios-material-*` or `Material` (a `--material-*` background with
  `--material-blur`), never an `rgba()` background and a `blur()` of an app's own, so it follows the theme and Reduce
  Transparency.
- **Why (iPadOS):** Materials; Accessibility.
- **Check:** `auto`: a computed `backdrop-filter` other than `none` and other than the resolved `--material-blur`; or an
  element with a background alpha between 0 and 1 that is not a `--material-*`, `--fill*`, `--separator*` or
  `--dimming` value; or, with `.reduce-transparency` on the root, any element with a `backdrop-filter` other than
  `none`, or a bar, menu, popover or sheet whose background alpha is below 1.
- **Wrong / Right:** `background: rgb(255 255 255 / .6); backdrop-filter: blur(12px)` / `class="ios-material-thick"`

### MOTION-04 Bars are clear until content scrolls under them
- **Rule:** A navigation bar has no background at the scroll edge and takes the chrome material and its hairline once
  content scrolls beneath it (as `NavigationBar` does); it is never an opaque coloured band.
- **Why (iPadOS):** Toolbars; Materials.
- **Check:** `auto`: a bar whose background is opaque (alpha 1) and not transparent at `scrollTop` 0 of its scroller.
- **Wrong / Right:** a solid blue header bar / a clear bar that frosts on scroll.

### MOTION-05 A dim is only a dim
- **Rule:** What sits behind a modal is dimmed with `--dimming` and never blurred; blur behind a dialog is Spotlight's
  alone.
- **Why (iPadOS):** Modality; Alerts.
- **Check:** `auto`: an open `dialog`'s `::backdrop`, or a scrim, with a `backdrop-filter` other than `none`, outside
  `Spotlight`.
- **Wrong / Right:** `dialog::backdrop { backdrop-filter: blur(8px) }` / `background: var(--dimming)`.

## Motion

### MOTION-06 Durations are the tokens
- **Rule:** Every transition and animation an app writes lasts `--duration-fast`, `--duration` or `--duration-slow`
  (180, 300 or 450 ms); nothing longer.
- **Why (iPadOS):** Motion.
- **Check:** `auto`: on app elements, a computed `transition-duration` or `animation-duration` (per listed property)
  not in {0s, 180ms, 300ms, 450ms}.
- **Wrong / Right:** `transition: opacity .25s` / `transition: opacity var(--duration-fast) ease`

### MOTION-07 Movement eases like iOS
- **Rule:** Movement (transform, position, size) uses `--ease-ios`, or `--ease-spring` for a deliberate small
  overshoot; opacity and colour may use `ease`. Never `linear` movement or a bounce of an app's own.
- **Why (iPadOS):** Motion.
- **Check:** `auto`: an app transition on `transform`, `translate`, `scale`, `width`, `height`, `top`, `left`, `inset`
  whose timing function is not `cubic-bezier(0.32, 0.72, 0, 1)` or `cubic-bezier(0.34, 1.36, 0.64, 1)`.
- **Wrong / Right:** `transition: transform 300ms linear` / `transition: transform var(--duration) var(--ease-ios)`

### MOTION-08 Nothing loops but progress
- **Rule:** No animation repeats forever except a busy indicator; nothing pulses, floats, shimmers or bounces to draw
  attention. (That a busy indicator is the kit's is FEED-04.)
- **Why (iPadOS):** Motion; Accessibility.
- **Check:** `auto`: an element with `animation-iteration-count: infinite` that is not a busy indicator: not inside
  `ui-spinner` or `ActivityIndicator`, not `role="progressbar"` or `role="status"`, and with keyframes that change
  something other than `transform: rotate`.
- **Wrong / Right:** a pulsing "New" dot / a static one with text.

### MOTION-09 Frequent actions don't animate
- **Rule:** Things people do many times (typing, selecting a row, hovering, toggling a filter) change at once or with
  the kit's own short state change; no extra movement of an app's own.
- **Why (iPadOS):** Motion.
- **Check:** `interact`: select, toggle and hover a few times; a still picture cannot show it.
- **Wrong / Right:** each selected row slides and scales / the row highlights.

### MOTION-10 Motion follows where things go
- **Rule:** A view leaves the way it came (pushed from the trailing edge, back to it); a popover grows from its
  anchor; a sheet rises and falls; a removed row collapses in place.
- **Why (iPadOS):** Motion.
- **Check:** `interact`: push, go back, open and close, and watch the way each leaves.
- **Wrong / Right:** a detail that slides in from the right and fades out upward / it slides back to the right.

### MOTION-11 Nobody waits for an animation
- **Rule:** Input is accepted during a transition (a click mid-animation lands, a new navigation interrupts the old),
  and no transition blocks the next action.
- **Why (iPadOS):** Motion.
- **Check:** `interact`: click the next control while a view transition runs.
- **Wrong / Right:** buttons ignored until a 450 ms push finishes / the push is interrupted by Back.

### MOTION-12 Motion is never the message
- **Rule:** Information carried by motion (a shake for a wrong value, a bounce for new content) is also said in text or
  a glyph, so it survives Reduce Motion.
- **Why (iPadOS):** Motion.
- **Check:** `look` with Reduce Motion on.
- **Wrong / Right:** a field that only shakes on a wrong port / it also turns red with a footer.
