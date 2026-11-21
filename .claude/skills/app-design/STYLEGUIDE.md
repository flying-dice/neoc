# vos styleguide

iPadOS virtual desktop, **flat design** in the iOS 7 to iOS 17 sense (before Liquid Glass): colour, type and spacing do the
work, not texture, gloss or 3D. This is the contract that [CONFORMANCE.md](CONFORMANCE.md) scores and the review
checklist in section 11 checks. Tokens live in `packages/ui/src/styles/tokens.css` (the source of truth for values),
components in `packages/ui`. If a screen disagrees with this file, the screen is wrong, or this file is changed
deliberately in the same PR (say so in the round report).

## 1. Principles

1. Content first, chrome recedes: bars, toolbars and the Dock are quiet; colour goes to content and the one primary action.
2. Flat surfaces: every surface is a solid token colour. No bevels, inner glows, gloss, textures or skeuomorphism.
3. Depth by layer: wallpaper, then windows, then transient UI (menus, popovers, alerts, sheets); one elevation shadow each.
4. One accent: interactive things use `--tint`; destructive actions use `--ios-red`. Nothing else is "clickable colour".
5. System type, Dynamic Type scale: only the type tokens. No custom font sizes in app code.
6. Light and dark are equal: every colour is a token declared with `light-dark()`.
7. Accessible by default: WCAG 2.1 AA contrast, 44 pt targets, visible focus, full keyboard use, a label on every control.

## 2. Colour

Use system tokens for meaning, never a near-miss hex: `--ios-blue` (accent, links), `--ios-red` (destructive, errors,
badges), `--ios-green` (success, on), `--ios-orange` (warnings), the other `--ios-*` and `--ios-gray*` for icon tiles and
categories. Semantic groups: labels (`--label` to `--label-quaternary`; tertiary is never text people must read), fills,
backgrounds and grouped backgrounds, `--bg-elevated` for windows and sheets, separators. `--focus-ring` is always system
blue and holds 3:1 on every body in both themes (`packages/ui/src/contrast.test.ts`); it never follows `--tint`.

Allowed exceptions: the wallpaper (the only full-bleed gradient); app icons and tiles (solid, or a vertical two-stop
gradient of one hue, no gloss); an app's own canvas when it is a recognisable instrument (the Calculator's black body and
orange keys), which must still meet contrast and declare `color-scheme`. Banned: gloss, inner shadows on controls, glows,
`text-shadow` on content, more than one accent on a screen.

## 3. Materials

Blurred translucency only for chrome floating over varied content (`--material-chrome`, `-thin`, `-regular`, `-thick`,
`-ultrathin`), always with `--material-blur`. Reduce Transparency swaps every material for an opaque background, so never
hard-code an `rgba()` on chrome. Content surfaces (rows, cards, bubbles, window bodies) are never translucent.

## 4. Typography

The `--type-*` tokens (large-title 34 down to caption2 11; body 17). Use the `Text` atom or the tokens; no raw
`font-size` (an instrument display is the exception). Weights regular to bold; nothing lighter than regular under 20 px.
Text scales with Settings, so layouts survive 85% to 125%. Numbers that change in place use tabular figures.

## 5. Layout, spacing, radii, elevation

- 4 pt grid (4, 8, 12, 16, 20, 24, 32); row inset 16, row height 44 minimum; hit targets at least 44x44 (28 px only for
  compact bar buttons with 44 px of tappable area). The one off-grid inset is a wide key's label (Calculator `0`, 28 px).
- Readable width caps at about 720 px (settings) or 980 px (Discover). Inset-grouped lists are the default for structured
  data. Sidebars 280 to 320 px; below 700 px of window width a split view collapses to its detail column.
- Radii only from the scale (`--radius-xs` to `--radius-2xl`, `-pill`, `-dock`, `-icon`); nested corners shrink by the padding.
- Shadows only from the elevation tokens (`--shadow-card`, `-window`, `-window-inactive`, `-slideover`, `-popover`,
  `-control`), on the plane they belong to.

## 6. Iconography

Line glyphs from `packages/ui/src/icons.ts` through the `Icon` atom, sized with their text. No emoji as UI icons. An
icon-only button needs a `label`. App icons are a white glyph on a squircle tile (`IconTile`).

## 7. The desktop shell

Wallpaper, a 28 px status bar, a left recent-apps strip, a stage of floating windows, a floating Dock, widgets. A window
has a "···" multitasking handle (Full Screen, Split View, Slide Over, Minimize, Close; keyboard-operable, focus returns to
the handle) and a resize grip that is always visible, keyboard-resizable and says when the minimum or maximum is reached
(also to a screen reader). The minimum window is 320x240. Split View and Slide Over that cannot fit an app's minimum width
are shown unavailable (`aria-disabled`, still focusable, with the reason), never hidden or refused silently; a screen that
shrinks below the minimums dissolves them into floating windows. Motion respects `prefers-reduced-motion`.
`e2e/desktop.spec.ts` and `client/journeys/desktop.md` pin the behaviour.

## 8. Inside an app

An app window is an iPadOS scene built from the templates (`ScreenTemplate`, `SplitViewTemplate`). The navigation bar title
says where you are. One primary action (a filled `Button` or a trailing bar button); the rest tinted, gray or plain. Lists,
not cards, for structured data. Empty states: centred, a headline, one line, at most one action. Destructive or
irreversible actions confirm with an `Alert`. Content stays usable at the minimum size and never scrolls sideways: wrap,
truncate with an ellipsis, or collapse the sidebar.

## 9. Controls

Action: `Button` (`filled` primary, `tinted`, `gray`, `plain`), `IconButton` in bars. On/off: `Toggle`. One of a few:
`SegmentedControl` (2 to 5). One of many: a `ListSection` with a checkmark. Range: `Slider`. Text: `TextInput`,
`TextFieldRow`, `SearchField`. Choice of actions: `Menu`, `Alert` (2 or 3 actions, cancel bold), `Sheet`. Progress:
`ActivityIndicator`, `ProgressBar`. Every control is reachable with Tab, shows the shared `:focus-visible` ring (3 px
`--focus-ring`; `--label` inside a tint fill), works with Space and Enter and has a `data-testid`. A control that cannot be
used right now because of the screen is `aria-disabled` and stays focusable; a menu or toolbar never ignores a press.

## 10. Not part of this language

macOS chrome (traffic lights, a menu bar), Liquid Glass, Windows or Material patterns (FABs, ripples, hamburger menus),
skeuomorphism.

## 11. Review checklist

When you look at a screenshot, check:

- [ ] Every colour comes from a token and works in light **and** dark.
- [ ] Flat: no gloss, bevel, inner shadow or texture. Only elevation-scale shadows.
- [ ] Materials only on floating chrome, always blurred.
- [ ] Type is on the scale. Nothing clipped, nothing overlapping, ellipses where text can be long.
- [ ] One accent colour. Destructive actions in red.
- [ ] Spacing on the 4 pt grid, lists inset 16, corners from the radius scale.
- [ ] The window works at 320x240, at its preferred size and full screen, with no horizontal scroll.
- [ ] Focus ring (`--focus-ring`, 3 px) visible when tabbing, on every surface. Every control has a label and a `data-testid`.
