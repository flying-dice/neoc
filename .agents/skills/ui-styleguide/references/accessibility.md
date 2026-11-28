# Accessibility

vos must be usable by sight, by keyboard alone, by screen reader, with large text, with motion and transparency
reduced, and by people who cannot distinguish colours. The kit and the shell already carry roles, names, focus rings,
44 pt hit areas, focus traps and the Reduce Motion reset; most failures come from an app working around them.

Rules that live in other files and are part of this checklist too:

| Topic | Rules |
| --- | --- |
| Contrast (text 4.5:1, large text 3:1, glyphs and control edges 3:1, placeholders) | COLOR-14, COLOR-15, COLOR-16, COLOR-17 |
| Colour is never the only signal | COLOR-18 |
| Text follows Text Size; minimum size | TYPE-02, TYPE-04 |
| Heading order | TYPE-47 |
| Status and errors announced | FEED-20, FORM-21 |
| Modal focus trap and return | MODAL-04 |
| Reduce Transparency: materials from tokens, opaque when it is on | MOTION-03 |
| Motion is never the only message | MOTION-12 |
| Swipe actions reachable another way | LIST-14 |
| Shortcuts, tab stops per group, Return, Escape, focus following the view | INPUT-04 to INPUT-10 |

---

## Targets

### A11Y-01 Every target is at least 44 × 44
- **Rule:** Every control's hit area is at least 44 × 44 px: kit controls get it from `hit` or from sitting in a
  `ui-row` (which sets `--ui-hit`), shell controls from their own 44 pt boxes. Links inside running text are exempt,
  and list rows are LAYOUT-03's.
- **Why (iPadOS):** Accessibility; Buttons.
- **Check:** `auto`: an interactive element (see auto-checks.md for the selector) whose hit box is narrower or shorter
  than 44px (−0.5px tolerance); for a kit element the hit box is its host's border box.
- **Wrong / Right:** `<ui-button square size="small">` alone in a toolbar (28 px) / `<ui-button square size="small" hit>`

### A11Y-02 Targets do not overlap
- **Rule:** No two controls' hit areas overlap, so a press always lands on one control.
- **Why (iPadOS):** Accessibility.
- **Check:** `auto`: the hit boxes of two interactive elements, neither containing the other, intersect by more than
  1px in both axes.
- **Wrong / Right:** two 44 px glyph buttons 30 px apart, centre to centre / 44 px apart.

## Names and roles

### A11Y-03 Every control has a name
- **Rule:** Every interactive element has a non-empty accessible name: its visible text, or `label` on a kit element
  (which sets `aria-label` inside its shadow root).
- **Why (iPadOS):** Accessibility.
- **Check:** `auto`: an interactive element (crossing shadow roots) whose computed accessible name is empty.
- **Wrong / Right:** `<ui-toggle slot="trailing">` in a row titled by slotted markup / `<ui-toggle slot="trailing" label="Sync">`

### A11Y-04 A glyph-only control has a real name
- **Rule:** A control that shows only a glyph is named for its action in words (`Delete`, `More`), never by the glyph's
  file name, the word "button" or "icon", or a symbol character.
- **Why (iPadOS):** Icons; Accessibility.
- **Check:** `auto`: an interactive element whose visible content is only an icon and whose (non-empty; empty is
  A11Y-03) accessible name matches `/^(button|icon|image|img|svg|graphic|\W+|[a-z0-9]+(\.[a-z0-9]+)+)$/i` (a symbol
  name like `square.and.arrow.up`).
- **Wrong / Right:** `aria-label="ellipsis.circle"` / `label="More"`

### A11Y-05 The name contains the visible label
- **Rule:** A control with visible text has an accessible name that contains that text (so Voice Control can say
  it), and never includes the control type ("Save button").
- **Why (iPadOS):** Accessibility.
- **Check:** `auto`: an interactive element with visible text whose accessible name does not contain that text
  (case-insensitive), or whose name ends in `button`, `link`, `switch` or `checkbox`.
- **Wrong / Right:** visible `Restart`, `aria-label="Reboot container button"` / no `aria-label`, or
  `aria-label="Restart web"`.

### A11Y-06 What can be clicked is a control
- **Rule:** Anything that reacts to a click is a real control (a kit element, a `button`, an `a[href]`, or an element
  with a role and `tabindex="0"` and keyboard handling); never a bare `div` or `span` with a click handler.
- **Why (iPadOS):** Accessibility.
- **Check:** `auto`: an element with computed `cursor: pointer` that is not focusable and has no interactive ancestor.
- **Wrong / Right:** `<div class="card" onclick=…>` / `<ui-row tappable>` or a `button`.

### A11Y-07 State is exposed, not only drawn
- **Rule:** A control's state is in its semantics: `aria-pressed`, `aria-checked`, `aria-selected`, `aria-expanded`,
  `aria-disabled` / `disabled`, `aria-invalid`, as the kit sets them. An app does not draw a state the tree lacks.
- **Why (iPadOS):** Accessibility; Toggles.
- **Check:** `auto`: an element with `aria-haspopup` lacking `aria-expanded` while its menu is open; a visually
  selected row (tint fill) without `aria-selected="true"`.
- **Wrong / Right:** a chosen segment shown only by a class / `aria-checked="true"`.

### A11Y-08 Decorative images are hidden, meaningful ones described
- **Rule:** An `img` always has `alt`: empty for decoration, a description for content; a decorative glyph is
  `aria-hidden` (an unlabelled `ui-icon` is).
- **Why (iPadOS):** Accessibility; Images.
- **Check:** `auto`: an `img` without an `alt` attribute; an `svg` with no name that is not `aria-hidden` and is the
  only content of nothing interactive.
- **Wrong / Right:** `<img src="avatar.png">` / `<img src="avatar.png" alt="">` beside the person's name.

### A11Y-09 Reading order is visual order
- **Rule:** The DOM order of content and controls is the order they appear on screen; CSS (`order`, `*-reverse`,
  absolute placement) never reorders interactive content.
- **Why (iPadOS):** Accessibility; Layout.
- **Check:** `auto`: an element with computed `order` other than 0, or a flex container with `row-reverse` or
  `column-reverse`, that contains a focusable element.
- **Wrong / Right:** a Cancel button visually leading but last in the DOM / DOM order Cancel, Done.

### A11Y-10 The document says its language
- **Rule:** Every page (the shell and each app's page) sets `lang` on its root element.
- **Why (iPadOS):** Accessibility; Writing.
- **Check:** `auto`: `document.documentElement.lang` is empty.
- **Wrong / Right:** `<html>` / `<html lang="en">`

## Focus and keyboard

### A11Y-11 Focus is always visible
- **Rule:** Every element that takes keyboard focus shows the 3 px `--focus-ring` outline (the kit's and base.css's
  `:focus-visible` ring; `--label` on a tint-filled selected row), unclipped. App CSS never removes it
  (`outline: none`) without drawing the same ring.
- **Why (iPadOS):** Accessibility; Keyboards.
- **Check:** `auto`: after focusing each element by Tab, neither it, its focused inner element nor its `ui-row` face has
  an outline of at least 2px whose colour is `--focus-ring` or `--label`, or the ring is clipped by an
  `overflow: hidden` ancestor (the outline box extends past the ancestor's padding box); `source`: `outline: none|0`
  in a `:focus` or `:focus-visible` rule in `apps/**`.
- **Wrong / Right:** a row with `overflow: hidden` that cuts the ring off / the ring drawn inside (`outline-offset: -2px`).

### A11Y-12 No positive tabindex
- **Rule:** Focus order comes from the DOM and the kit's roving tab stops; no element has a `tabindex` above 0.
- **Why (iPadOS):** Text fields; Accessibility.
- **Check:** `auto`: an element (crossing shadow roots) with `tabIndex > 0`.
- **Wrong / Right:** `tabindex="3"` on the Save button / DOM order.

### A11Y-13 Everything works from the keyboard
- **Rule:** Every action can be done with the keyboard alone (Tab, arrows, Return, Space, Escape, the Menu key);
  nothing needs a pointer, a drag or a hover.
- **Why (iPadOS):** Accessibility.
- **Check:** `interact`: unplug the mouse and do each task.
- **Wrong / Right:** reordering only by dragging / also `Move Up` and `Move Down` in the context menu.

### A11Y-14 No keyboard traps
- **Rule:** Tab and Shift+Tab can always leave a region, except an open modal (which traps on purpose and closes with
  Escape).
- **Why (iPadOS):** Accessibility; Keyboards.
- **Check:** `auto`: pressing Tab 100 times from a focused element outside any modal never reaches an element outside
  the region (the smallest ancestor holding all the visited elements) when there are focusable elements outside it.
- **Wrong / Right:** a code editor that swallows Tab with no way out / Escape then Tab leaves it.

## Settings people choose

### A11Y-15 Under Reduce Motion nothing moves
- **Rule:** With Reduce Motion on, no transition or animation runs, in CSS or in script (Svelte transitions take their
  duration from `motion()`), except a busy indicator that keeps turning.
- **Why (iPadOS):** Accessibility; Motion.
- **Check:** `auto`: with `prefers-reduced-motion: reduce` emulated, an element (crossing shadow roots) with a computed
  `transition-duration` or `animation-duration` above 0s, or any entry of `document.getAnimations()` (and each shadow
  root's) with a duration above 0, other than `ui-spinner` and `ActivityIndicator`; or `scroll-behavior: smooth`.
- **Wrong / Right:** `in:fly={{ y: 20, duration: 300 }}` / `in:fly={{ y: 20, duration: motion(300) }}`

### A11Y-16 The largest Text Size still fits
- **Rule:** At the largest Text Size (`--text-scale` 1.25) every screen still shows all of its text without clipping or
  overlap, and nothing that was whole at the default size becomes cut off without an ellipsis.
- **Why (iPadOS):** Typography; Accessibility.
- **Check:** `auto`: at `--text-scale` 1.25, any failure of TYPE-26, TYPE-27 or TYPE-28.
- **Wrong / Right:** a fixed-height row whose text spills at 125% / a row that grows.

### A11Y-17 The page holds at 200% zoom
- **Rule:** At 200% browser zoom (and a window of 320 px) content reflows: nothing is lost, overlaps, or needs
  scrolling in two directions to read.
- **Why (iPadOS):** Accessibility.
- **Check:** `interact`.
- **Wrong / Right:** a fixed 900 px table that overflows the window / columns that collapse into rows.

## Hover, gestures and time

### A11Y-18 Hover is never required
- **Rule:** Anything revealed on hover (a control, a value, a hint) is also shown on keyboard focus and reachable by
  touch; essential controls are not hidden until hover.
- **Why (iPadOS):** Accessibility; Pointing devices.
- **Check:** `interact`; `auto`: an interactive element with `opacity: 0` or `visibility: hidden` that becomes visible on
  `:hover` but not on `:focus-within`.
- **Wrong / Right:** a row's delete button that appears only on hover / always present in the trailing slot or menu.

### A11Y-19 Gestures have plain alternatives
- **Rule:** Every long press, drag, swipe or pinch has an equivalent single click, menu item or key.
- **Why (iPadOS):** Accessibility.
- **Check:** `interact`.
- **Wrong / Right:** resize a split only by dragging / the split handle also responds to arrow keys.

### A11Y-20 Nothing needed disappears on a timer
- **Rule:** Nothing a person must read or act on goes away on a timer; a transient message pauses while pointed at or
  focused and can be dismissed.
- **Why (iPadOS):** Accessibility.
- **Check:** `interact`.
- **Wrong / Right:** an Undo toast that vanishes in 2 s even while hovered / it waits while hovered or focused.
