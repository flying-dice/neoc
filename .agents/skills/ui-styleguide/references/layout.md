# Layout

An app's page lives in a Stage Manager window that a person resizes freely, from 320 × 240 px up to the whole
screen. Lay it out for the window it is in, not the screen, with the kit's metrics: `--row-height` (44), `--row-inset`
(16), the radius and shadow tokens, and the column widths of `ui-split-view`.

## Metrics

| What | Value | Token or element |
| --- | --- | --- |
| Window content margin | 16 px compact, 20 px regular | `ui-section` (`--list-inset`, 20) |
| Row height (minimum) | 44 px | `--row-height` |
| Row inset (leading and trailing padding in a row) | 16 px | `--row-inset` |
| Navigation bar height | 50 px | `--nav-bar-height` |
| Gap between sections | 35 px (inset grouped), 22 px (sidebar) | `ui-section` |
| Sidebar width / middle column | 280 px (min 200) / 300 px (min 220) | `ui-split-view` |
| One column below | 700 px of the split view's own width | `ui-split-view` |
| Hairline | 0.5 px | `--hairline` |

| Surface | Radius | Shadow |
| --- | --- | --- |
| Window | `--radius-xl` (18) | `--shadow-window`, inactive `--shadow-window-inactive` |
| Alert, popover, context menu, toast, banner card | `--radius-lg` (14) | `--shadow-popover` |
| Inset-grouped section, search and text field, picker | `--radius-md` (10) | none |
| Button: small / medium / large | 7 / 8 / 12 (kit) | none |
| Small element (a tag, an inline code chip, a row icon tile) | `--radius-xs` (6) or `--radius-sm` (8) | none |
| Widget, platter (`Card`) | `--radius-2xl` (22) | `--shadow-card` |
| Summary tile (`ui-tile`) | `--radius-lg` (14) | `--shadow-card` |
| Capsule (a toggle track, a pill, a name label) | `--radius-pill` | none |
| Dock | `--radius-dock` | its own shadow today, not a token (a known LAYOUT-16 gap) |
| App icon | `--radius-icon` | none |
| Slide Over window | `--radius-xl` | `--shadow-slideover` |

---

## Margins and rhythm

### LAYOUT-01 Text keeps off the window's edge
- **Rule:** No text sits closer than 16px to the edge of the window's content area. Full-bleed lists, images and
  canvases may touch the edge; their text is still inset by `--row-inset`.
- **Why (iPadOS):** Layout.
- **Check:** `auto`: a text node's client rect whose left or right edge is within 16px of the window content
  element's left or right edge.
- **Wrong / Right:** `<p style="padding:4px">` at the top of a window / text inside a `ui-section` (20px inset) or with
  `padding-inline: 16px`.

### LAYOUT-02 Spacing is on the 4-point grid
- **Rule:** Margins, padding and gaps an app sets are 0, 1px, 2px or a multiple of 4px (8px the usual step);
  the kit's own UIKit metrics (7, 11, 35…) are the kit's, not a licence for an app.
- **Why (iPadOS):** Layout asks for consistent spacing and the system's margins but names no grid; the 4-point grid
  is vos's way of keeping an app's own spacing consistent.
- **Check:** `auto`: on elements outside kit and shell shadow roots, a computed `margin-*`, `padding-*`, `gap`,
  `row-gap` or `column-gap` whose absolute px value is not 0, 1, 2, a multiple of 4 (±0.5px), 16 (`--row-inset`) or
  0.5 (`--hairline`).
- **Wrong / Right:** `gap: 10px; padding: 7px 13px` / `gap: 8px; padding: 8px 12px`

### LAYOUT-03 Rows are at least 44 high
- **Rule:** Every list row, menu item and option is at least `--row-height` (44px) high and grows with its text; it is
  never given a fixed height.
- **Why (iPadOS):** Layout; Lists and tables; Accessibility.
- **Check:** `auto`: an element with role `listitem`, `option`, `row`, `menuitem`, `treeitem` or a `ui-row` whose height is
  under 44px; `interact` at the largest Text Size for growth.
- **Wrong / Right:** `.row { height: 32px }` / `<ui-row>` (min-height 44).

### LAYOUT-04 Row content is inset 16
- **Rule:** A row's leading icon or text starts `--row-inset` (16px) from the row's leading edge, and its trailing
  content ends 16px from the trailing edge.
- **Why (iPadOS):** Lists and tables.
- **Check:** `auto`: in a hand-made row (an element with row semantics outside `ui-row`/`ListRow`), the first content
  rect starts less than 15.5px or more than 16.5px from the row's leading edge.
- **Wrong / Right:** `.row { padding: 0 8px }` / `<ui-row>`, or `padding-inline: var(--row-inset)`.

### LAYOUT-05 Groups are separated by space, rows by hairlines
- **Rule:** Related items share a section with hairlines between rows; unrelated groups are separate sections with
  the section gap between them. Do not separate groups with rules, boxes or shadows.
- **Why (iPadOS):** Layout.
- **Check:** `look`.
- **Wrong / Right:** one long section with a bold divider in the middle / two `ui-section`s.

### LAYOUT-06 Things that belong together align
- **Rule:** Down a column, everything starts on one of two vertical lines: the column's margin, where its title, a
  search field and an inset group's card start; and `--row-inset` (16px) further in, where section headers, each
  row's first glyph or text, and footers start. In a plain list, whose rows run edge to edge, the two are one line:
  the title, the search field, the headers and the row text all start 16px in. Indentation only means "belongs to the
  item above".
- **Why (iPadOS):** Layout ("Align elements to make them easier to scan, and use indentation to convey hierarchy";
  respect the system's margins); the `--row-inset` token.
- **Check:** `look`: follow each column's left edges down the picture; an edge a few pixels off both lines fails.
  `auto`: in a hand-made section, the header's text starts more than 1px away from the first row's text (review: under
  rows that lead with a glyph, the header lines up with the glyph).
- **Wrong / Right:** a list title at 8px, its search field at 4px and its rows' text at 16px / all three at 16px.

### LAYOUT-07 The most important thing is top and leading
- **Rule:** Put the primary content and its title at the top leading corner; secondary panels and inspectors go
  trailing or below.
- **Why (iPadOS):** Layout.
- **Check:** `look`.
- **Wrong / Right:** a summary tile grid below a long log / the tiles first, the log below.

## Safe areas and windows

### LAYOUT-08 Nothing hides under window chrome
- **Rule:** No control or text of the app sits under the window's "···" multitasking handle (top centre) or its resize
  grip (bottom trailing corner), and the last row of a scrolling view can scroll clear of both.
- **Why (iPadOS):** Layout; Windows.
- **Check:** `auto`: an interactive element or text rect of the app that intersects the rect of `[data-testid$=-multitasking]`
  or `[data-testid$=-resize]` of its window.
- **Wrong / Right:** a toolbar button centred at the very top of the window / bar items at the leading and trailing
  ends, clear of the handle.

### LAYOUT-09 Lay out for the window, not the screen
- **Rule:** Layout decisions read the width of the app's own container (container queries, `ResizeObserver`,
  `ui-split-view`'s `collapsed`), never the viewport, the screen, the device or its orientation.
- **Why (iPadOS):** Layout.
- **Check:** `source`: in `apps/**`, `@media` with `width`/`orientation`, `window.innerWidth`, `screen.*` or
  `navigator.userAgent` used for layout.
- **Wrong / Right:** `@media (max-width: 700px)` / `@container (max-width: 700px)` or `ui-split-view`.

### LAYOUT-10 One shared compact width
- **Rule:** A layout goes to one column at the width the kit decides (`ui-split-view`, below 700px of its own width);
  an app does not invent its own breakpoint for the same change.
- **Why (iPadOS):** Layout; Split views.
- **Check:** `source`.
- **Wrong / Right:** a sidebar hidden below 600px by a hand-written query / `ui-split-view`.

### LAYOUT-11 Every function at every width
- **Rule:** Making the window narrower moves functions (into a menu, behind navigation), never removes them.
- **Why (iPadOS):** Layout.
- **Check:** `interact`: compare what can be done at 320px and at full width.
- **Wrong / Right:** the Export button disappears in a narrow window / it moves into the `…` menu.

### LAYOUT-12 The window holds together at its smallest
- **Rule:** At the window's minimum size (320 × 240 px) and at full screen, nothing overlaps, clips or forces the
  window's content to scroll sideways; only a deliberate horizontal list or table scrolls in itself.
- **Why (iPadOS):** Layout; Windows; Multitasking.
- **Check:** `auto`: at a window of 320 × 240 px, the window content root has `scrollWidth > clientWidth + 1`, or
  TYPE-27/TYPE-28 fail.
- **Wrong / Right:** a form that needs 480px and scrolls sideways / a form that wraps its rows.

## Columns

### LAYOUT-13 Columns are the split view's
- **Rule:** A sidebar beside a detail, or sidebar, list and detail, is `ui-split-view` (or `SplitViewTemplate` in the
  shell) with its default column widths; an app does not build its own columns or set `--ui-split-*` per screen.
- **Why (iPadOS):** Split views; Sidebars.
- **Check:** `source`: a hand-made two-column grid with a list on the leading side, or `--ui-split-sidebar-width` set
  in an app.
- **Wrong / Right:** `display:grid; grid-template-columns: 240px 1fr` with a list / `<ui-split-view>`.

## Radii and elevation

### LAYOUT-14 Radii come from the tokens
- **Rule:** A rounded shape uses the radius of its surface (table above): a token, the kit button radii, or fully
  round (a capsule or circle). No other radius.
- **Why (iPadOS):** Layout; Materials.
- **Check:** `auto`: on app elements, a computed `border-*-radius` (px) not in {0, 6, 7, 8, 10, 12, 14, 18, 22, 30}
  and not ≥ half the element's shorter side; or a `%` radius other than 50% and 22.37%.
- **Wrong / Right:** `border-radius: 5px` / `border-radius: var(--radius-xs)`

### LAYOUT-15 Nested corners are concentric
- **Rule:** A rounded shape inside another keeps an even gap: its radius is the outer radius minus the gap (never a
  larger inner radius).
- **Why (iPadOS):** Layout; Apple's apps.
- **Check:** `auto`: an element with radius r inside a rounded ancestor of radius R at gap g < R, where r > R − g + 1.
- **Wrong / Right:** a 10px-radius button 4px inside a 10px card / a 6px-radius button there.

### LAYOUT-16 Shadows come from the tokens
- **Rule:** A shadow is one of the shadow tokens, on the surface it is named for: windows, popovers and menus, cards
  and tiles, control knobs. Nothing else casts a shadow: inside a window, lists, sections, fields and panels are flat.
- **Why (iPadOS):** Layout; Materials.
- **Check:** `auto`: an app element whose computed `box-shadow` is not `none`, a separator hairline
  (`0 0.5px 0 <separator>`), an inset outline, or one of `--shadow-control`, `--shadow-card`, `--shadow-popover`,
  `--shadow-window`, `--shadow-window-inactive`, `--shadow-slideover`.
- **Wrong / Right:** `box-shadow: 0 4px 12px rgba(0,0,0,.25)` on a list / no shadow.

## Containers and images

### LAYOUT-17 No box inside a box
- **Rule:** Group with padding, alignment and the section structure; never nest one bordered or filled container
  inside another to make a subgroup.
- **Why (iPadOS):** Boxes; Layout.
- **Check:** `auto`: an app element with a visible border or a non-transparent background different from its parent's,
  and a radius, inside another such element that is not a window, sheet, popover or `ui-section` rows box.
- **Wrong / Right:** a bordered card holding two more bordered cards / one section with two groups of rows.

### LAYOUT-18 Images keep their shape
- **Rule:** An image is never stretched: it keeps its aspect ratio (`object-fit: cover` or `contain`, or a box of the
  same ratio).
- **Why (iPadOS):** Images; Layout.
- **Check:** `auto`: an `img`, `video` or `canvas` with computed `object-fit: fill` whose rendered aspect ratio differs
  from its natural one by more than 1%.
- **Wrong / Right:** a 4:3 screenshot squeezed into a 16:9 box / `object-fit: cover`.

### LAYOUT-19 Images are sharp
- **Rule:** A bitmap is supplied at the screen's pixel density (2× at least), and icons are vectors (`ui-icon`).
- **Why (iPadOS):** Images.
- **Check:** `auto`: an `img` whose `naturalWidth` is below 0.9 × its rendered width × `devicePixelRatio` (SVG
  sources excepted).
- **Wrong / Right:** a 64 px PNG shown at 64 CSS px on a 2× screen / a 128 px PNG, or an SVG.
