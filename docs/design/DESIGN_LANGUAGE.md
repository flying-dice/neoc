# AgentOS design language

**iPadOS virtual desktop, flat design.**

AgentOS looks and behaves like an iPad running Stage Manager: a wallpaper, a status bar, a floating Dock, a
recent-apps strip on the left and app windows that float, go full screen, split or slide over. Inside each window,
an app is an iPadOS app: navigation bars, inset-grouped lists, sidebars, sheets and alerts.

The style is **flat**, in the iOS 7 to iOS 17 sense (before Liquid Glass): colour, type and spacing do the work, not
texture, gloss or 3D effects. Depth comes from layering and one small set of shadows. Translucency is only for
system chrome that floats over other content.

This document is the contract. Tokens live in `packages/ui/src/styles/tokens.css`, components in `packages/ui`.
If a screen disagrees with this document, the screen is wrong (or this document needs a deliberate update in the
same PR).

---

## 1. Principles

1. **Content first, chrome recedes.** Bars, toolbars and the Dock are quiet: neutral materials, thin hairlines,
   small labels. Colour goes to content and to the one primary action.
2. **Flat surfaces.** Every surface is a solid token colour. No bevels, inner glows, glossy highlights, textures,
   embossing, long shadows or skeuomorphic materials (leather, paper, metal).
3. **Depth by layer, not by decoration.** Three planes: wallpaper, then windows (and widgets), then transient UI
   (menus, popovers, alerts, sheets). Each plane has one shadow from the elevation scale. Nothing else casts one.
4. **One accent.** Interactive things use `--tint` (the user's accent colour, blue by default). Destructive actions
   use `--ios-red`. Nothing else is "clickable colour".
5. **System type, Dynamic Type scale.** Only the type tokens below. No custom font sizes in app code.
6. **Light and dark are equal citizens.** Every colour is a token declared with `light-dark()`. Never hard-code a
   colour that only works on one theme.
7. **Accessible by default.** WCAG 2.1 AA contrast, 44 pt hit targets, visible focus, full keyboard use, labels for
   every control. These are checked by automated tests (see `docs/TESTING.md`).

## 2. Colour

### System colours

Use the system palette for meaning, never a near-miss hex:

| Token | Use |
|---|---|
| `--ios-blue` | default accent, links, selection |
| `--ios-red` | destructive actions, errors, notification badges |
| `--ios-green` | success, "on", running |
| `--ios-orange` | warnings, "waiting for you" |
| `--ios-yellow`, `--ios-purple`, `--ios-pink`, `--ios-indigo`, `--ios-teal`, `--ios-mint`, `--ios-cyan`, `--ios-brown` | icon tiles, categories, charts |
| `--ios-gray` … `--ios-gray6` | neutral icon tiles and fills |

`--tint` is the accent and can be re-pointed per subtree (Settings → Accent Colour sets it on the root).

### Semantic colours

| Group | Tokens | Rule |
|---|---|---|
| Labels | `--label`, `--label-secondary`, `--label-tertiary`, `--label-quaternary` | primary text, then secondary info, then placeholders and disabled. Never use tertiary for text people must read. |
| Fills | `--fill` … `--fill-quaternary` | small shapes on top of a background: search fields, gray buttons, slider tracks |
| Backgrounds | `--bg`, `--bg-secondary`, `--bg-tertiary` | plain screens (chat thread, terminal, hero pages) |
| Grouped backgrounds | `--bg-grouped`, `--bg-grouped-secondary`, `--bg-grouped-tertiary` | inset-grouped list screens (Settings, Files) |
| Elevated | `--bg-elevated`, `--bg-grouped-elevated*` | windows and sheets (dark mode lifts them to gray6 so they separate from the wallpaper) |
| Separators | `--separator` (hairline), `--separator-opaque` | between rows and above bars |

### Allowed exceptions

- **Wallpaper** may be a photo or a multi-stop gradient. It is the only full-bleed gradient in the system.
- **App icons and icon tiles** are a solid colour, or a vertical two-stop gradient of **the same hue** (top up to
  ~15% lighter). No gloss, highlight band, rim light or texture.
- **An app's own canvas** may set its own fixed palette when the app is a recognisable instrument (Calculator's
  black body and orange operators, Terminal's dark scheme). It must still meet contrast and declare
  `color-scheme` so form controls match.

Banned: `IconTile gloss`-style highlights, inner shadows on controls, neon glows, `text-shadow` on content (the
status bar over wallpaper is the single exception), more than one accent on a screen.

## 3. Materials (translucency)

Materials are blurred, semi-transparent backgrounds for chrome floating over *moving or varied* content:

| Token | Use |
|---|---|
| `--material-chrome` | navigation bars and tab bars when content scrolls under them (`material` prop) |
| `--material-thin` | the Dock, the recent-apps strip thumbnails |
| `--material-regular` / `--material-thick` | menus, popovers, the multitasking menu, widgets |
| `--material-ultrathin` | hovering hints over the wallpaper |

Rules:

- Always pair a material with `--material-blur`. Never use a translucent fill without blur (it reads as a mistake).
- **Reduce Transparency** (Settings) swaps every material for an opaque elevated background. Components must use
  the tokens so this works; never hard-code an `rgba()` background on chrome.
- Content surfaces (list cells, cards, message bubbles, window bodies) are never translucent.

## 4. Typography

The system font stack (`--font-text`, `--font-display`), the Dynamic Type "Large" scale:

| Token | Size / leading | Use |
|---|---|---|
| `--type-large-title` | 34/41 bold | top of a scrolled-to-top root screen (`NavigationBar large`) |
| `--type-title1` … `--type-title3` | 28 / 22 / 20 | page and section headings |
| `--type-headline` | 17/22 semibold | row titles that need emphasis, alert titles |
| `--type-body` | 17/22 | default text, list rows |
| `--type-callout` | 16/21 | explanatory text in cards |
| `--type-subheadline` | 15/20 | secondary rows, buttons (medium) |
| `--type-footnote` | 13/18 | list footers, metadata |
| `--type-caption1` / `--type-caption2` | 12 / 11 | badges, labels under icons, timestamps |

- Use the `Text` atom (`style="…"`) or the type tokens. No raw `font-size` in app code (an instrument canvas like
  the Calculator display is the exception).
- Weights: regular, medium, semibold, bold. No light or thin weights below 20 px.
- Text scales with Settings → Text Size (the root `font-size`), so layouts must survive 85% to 125%.
- Numbers that change in place use `font-variant-numeric: tabular-nums`.

## 5. Layout and spacing

- **4 pt grid.** Spacing is 4, 8, 12, 16, 20, 24, 32. Row inset is `--row-inset` (16). Row height is
  `--row-height` (44) minimum.
- **Hit targets** are at least 44x44 (28 px is allowed only for compact buttons inside bars, with 44 px of
  surrounding tappable area).
- **Readable width.** Text-heavy content caps at ~720 px (Settings panes) or ~980 px (Discover) and centres.
- **Inset-grouped lists** are the default for settings and structured data: 16 px side margins, `--radius-md`
  corners, hairline separators inset to the text.
- **Sidebars** are 280 to 320 px. Below 700 px of window width a split view collapses to its detail column.

### Corner radii

| Token | Use |
|---|---|
| `--radius-xs` 6 / `--radius-sm` 8 | small buttons, badges, tags |
| `--radius-md` 10 | grouped lists, search fields, text fields |
| `--radius-lg` 14 | alerts, popovers, menus, cards |
| `--radius-xl` 18 | windows, the hero banner |
| `--radius-2xl` 22 | widgets |
| `--radius-dock` 30 | the Dock |
| `--radius-icon` | app icon squircle |

Nested corners shrink: a child inside a rounded parent uses `parent radius − padding`.

### Elevation

Exactly these shadows, by plane:

| Token | Plane |
|---|---|
| `--shadow-card` | cards resting on a background (optional) |
| `--shadow-window` / `--shadow-window-inactive` | focused / unfocused windows |
| `--shadow-popover` | menus, popovers, alerts, sheets |
| `--shadow-control` | moving thumbs (switch knob, slider thumb, segmented thumb) |

## 6. Iconography

- Icons are line glyphs from `packages/ui/src/icons.ts` drawn by the `Icon` atom: SF Symbols-like, 1.5 to 2 px
  strokes, round caps. Size them with the text they sit next to (17 px in rows, 22 px in bars).
- **No emoji** as UI icons.
- An icon that is the only content of a button needs a text label (`label` / `aria-label`).
- **App icons** are a white glyph on a squircle tile (`IconTile`), coloured per the rule in section 2.

## 7. The desktop shell

| Element | Behaviour |
|---|---|
| **Wallpaper** | full-bleed, behind everything. |
| **Status bar** (28 px) | time and date leading, indicators trailing, white text over the wallpaper. |
| **Recent-apps strip** | left edge, 150 px, one thumbnail per stage (app group). Hides when a window overlaps it or the stage is full screen, and peeks back when the pointer rests on the left edge. |
| **Stage** | the area for the current app group. Windows float, cascade from the centre, and never open under the strip or the Dock. |
| **Window** | `--radius-xl`, elevated background, focused/unfocused shadow, a "···" multitasking handle centred at the top, a corner-arc resize affordance bottom right. Minimum 320x240. |
| **Multitasking menu** | Full Screen, Split View (left/right), Slide Over, Minimize, Close. |
| **Split View** | two windows share the stage with a 10 px gap and a draggable divider (25% to 75%). |
| **Slide Over** | one 380 px window over the right edge; hides to a tab. |
| **Dock** | floating, centred at the bottom, `--material-thin`, 58 px icons, a running dot under open apps, badges in red, recents after a divider. Hides when the stage is immersive. |
| **Widgets** | top right on the wallpaper, `--radius-2xl`, regular material. |

Motion: layout changes glide with `--ease-ios` over `--duration-slow`. Live drags track the pointer with no easing.
Everything respects `prefers-reduced-motion`.

## 8. Inside an app

An app window is an iPadOS app scene. Build screens from the templates:

- `ScreenTemplate`: navigation bar, scrolling content, optional bottom bar.
- `SplitViewTemplate`: sidebar plus detail. The sidebar uses `ListSection sidebar` rows with tinted icons and a
  selected state; the detail has its own navigation bar.

Rules:

- The navigation bar title says where you are. Root screens may use the large title.
- Primary action: one filled `Button`, or a trailing bar button. Everything else is tinted, gray or plain.
- Lists, not cards, for structured data. Cards are for promotional or summary content (Discover).
- Empty states: centred, a headline, one line of secondary text, at most one action.
- Destructive or irreversible actions confirm with an `Alert`.
- The content must stay usable at the window's minimum size (320x240) and look right at its preferred size. It
  must never scroll horizontally: wrap, truncate with an ellipsis, or collapse the sidebar.

## 9. Controls

| Need | Component |
|---|---|
| Action | `Button` (`filled` primary, `tinted` secondary, `gray` neutral, `plain` inline), `IconButton` in bars |
| On/off | `Toggle` (green when on) |
| One of a few | `SegmentedControl` (2 to 5 short options) |
| One of many | a `ListSection` with a checkmark on the chosen row |
| Value in a range | `Slider` / `IconSlider` |
| Text | `TextInput`, `TextFieldRow`, `SearchField` |
| Choice of actions | `Menu`, `Alert` (2 or 3 actions, cancel is bold), `Sheet` |
| Progress | `ActivityIndicator` (unknown), `ProgressBar` (known) |

Every control is reachable with Tab, shows the shared `:focus-visible` ring, works with Space/Enter, and has a
`data-testid` (see `docs/TESTING.md`).

## 10. Not part of this language

macOS chrome (traffic lights, a menu bar, pull-down menus, a compact Dock), Liquid Glass (refraction, specular
edges), Windows/Material Design patterns (FABs, ripples, hamburger menus) and skeuomorphism.

## 11. Review checklist

When you look at a screenshot, check:

- [ ] Every colour comes from a token and works in light **and** dark.
- [ ] Flat: no gloss, bevel, inner shadow or texture. Only elevation-scale shadows.
- [ ] Materials only on floating chrome, always blurred.
- [ ] Type is on the scale. Nothing clipped, nothing overlapping, ellipses where text can be long.
- [ ] One accent colour. Destructive actions in red.
- [ ] Spacing on the 4 pt grid, lists inset 16, corners from the radius scale.
- [ ] The window works at 320x240, at its preferred size and full screen, with no horizontal scroll.
- [ ] Focus ring visible when tabbing. Every control has a label and a `data-testid`.
