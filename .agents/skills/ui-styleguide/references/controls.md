# Controls

Every control an app shows is a kit element (`ui-button`, `ui-toggle`, `ui-check`, `ui-segmented`, `ui-picker`,
`ui-text-field`, `ui-text-area`, `ui-meter`, `ui-icon`); the shell uses its atoms and molecules (`Button`,
`IconButton`, `Toggle`, `SegmentedControl`, `Slider`, `IconSlider`, `Icon`, `ContextMenu`). They already have the
iPadOS look, the pressed and disabled states, the 44 pt hit area (`hit`, or automatically in a `ui-row`), the
keyboard behaviour and the accessible role. When a control is missing (a stepper, a slider in the kit, a pull-down
button, a disclosure row, a page control), add it to the kit; restyling a `<button>` or a `<div>` in one app is a
violation even when it looks right.

## Choosing the control

| Need | Control |
| --- | --- |
| Do something now | `ui-button` |
| On/off for a setting, in a list row | `ui-toggle` in the row's `trailing` slot |
| On/off outside a list (a filter, a mode) | `ui-button toggle` |
| Mark an item done, select rows | `ui-check`, or a list with `selection="multi"` |
| One of 2 to 5 short choices, all visible | `ui-segmented` |
| One of many, or of a changing set | `ui-picker` (a pop-up button) |
| One of many, long, or with explanations | a pushed list with a checkmark on the chosen row |
| Several actions behind one button | a pull-down (More `ellipsis.circle`, Add, Sort) |
| A value in a continuous range | a slider (shell `Slider`; the kit has none yet) |
| A small integer step | a stepper next to the value (none yet) |
| How full something is | `ui-meter` |

## Standard glyphs

| Action | Glyph name |
| --- | --- |
| Add | `plus` |
| Share, export | `square.and.arrow.up` |
| Delete | `trash` |
| More | `ellipsis.circle` (in a bar), `ellipsis` (in a row) |
| Search | `magnifyingglass` |
| Filter | `line.3.horizontal.decrease.circle` |
| Rename | `pencil` |
| Compose, new document | `square.and.pencil` |
| Duplicate | `plus.square.on.square` |
| Move to a folder | `folder` |
| Undo / Redo | `arrow.uturn.backward` / `arrow.uturn.forward` |
| Reload, restart | `arrow.clockwise` |
| Info | `info.circle` |
| Settings | `gearshape` |
| Select | `checkmark.circle` |
| Archive | `archivebox` |

---

## Buttons

### CTRL-01 Buttons are kit buttons
- **Rule:** An app's buttons are `ui-button` (the shell's `Button` or `IconButton`); never a native `button`, `a` or
  `div` styled to look like one.
- **Why (iPadOS):** Buttons.
- **Check:** `auto`: a `button`, `[role=button]` or `a` with a non-transparent background, a border or a radius, in an
  app's light DOM outside kit and shell components; `source`.
- **Wrong / Right:** `<button class="btn-primary">Save</button>` / `<ui-button>Save</ui-button>`

### CTRL-02 One prominent button per view
- **Rule:** A view (a window's content, a sheet, a popover) shows at most one `filled` button, for the action most
  people will take; the others are `tinted`, `gray` or `plain`. `filled` is `ui-button`'s default, so set `variant`
  on the rest.
- **Why (iPadOS):** Buttons.
- **Check:** `auto`: more than one visible `ui-button` whose computed `variant` is `filled` in one window content root
  or dialog.
- **Wrong / Right:** `<ui-button>Restart</ui-button><ui-button>Stop</ui-button>` / `<ui-button>Restart</ui-button>
  <ui-button variant="gray">Stop</ui-button>`

### CTRL-03 Style, not size, ranks a set
- **Rule:** Buttons offered together are one size; the preferred one is marked by a more prominent style.
- **Why (iPadOS):** Buttons.
- **Check:** `auto`: two or more sibling `ui-button`s (same parent, laid out in one row) with different `size`.
- **Wrong / Right:** a large `Continue` beside a small `Not Now` / both medium, `Continue` filled and `Not Now` plain.

### CTRL-04 A destructive button is never the prominent one
- **Rule:** A `destructive` button is `tinted`, `gray` or `plain`, never `filled`, and never takes the initial focus or
  Return.
- **Why (iPadOS):** Buttons.
- **Check:** `auto`: a `ui-button[destructive]` whose `variant` is `filled` (or unset); a destructive button that is
  `document.activeElement` when its view opens.
- **Wrong / Right:** `<ui-button destructive>Delete</ui-button>` / `<ui-button destructive variant="tinted">Delete</ui-button>`

### CTRL-05 Sizes have places
- **Rule:** `medium` (34 pt) is the default; `small` (28 pt) for buttons inside a row or banner; `large` (50 pt) only
  for the single main action of an empty state, onboarding or a sheet's bottom, full width of its column.
- **Why (iPadOS):** Buttons; Layout.
- **Check:** `look`.
- **Wrong / Right:** a row of large buttons in a toolbar area / one large `Add Container` in an empty state.

### CTRL-06 A slow button says it is working
- **Rule:** A button whose action takes more than about a second disables itself and shows a `ui-spinner` beside its
  label (which may change to an `…` form, `Saving…`) until the action ends.
- **Why (iPadOS):** Buttons.
- **Check:** `interact` (throttle the network).
- **Wrong / Right:** `Save` that looks idle for five seconds / `Saving…` with a spinner, disabled.

## Toggles and checks

### CTRL-07 A switch lives in a list row
- **Rule:** `ui-toggle` (`Toggle`) is only the trailing control of a list row whose title names the setting; outside a
  list, an on/off control is `ui-button toggle`.
- **Why (iPadOS):** Toggles.
- **Check:** `auto`: a `ui-toggle` or `[role=switch]` with no `ui-row` or `ListRow` ancestor.
- **Wrong / Right:** a switch floating in a toolbar for "Show hidden" / a `ui-button toggle` with a glyph.

### CTRL-08 A switch has no On/Off text
- **Rule:** The row's title says what the switch controls; the row does not also show `On` or `Off` as a value or a
  label next to the switch.
- **Why (iPadOS):** Toggles.
- **Check:** `auto`: a `ui-row` containing a `ui-toggle` whose `value` (or trailing text) is `On`, `Off`, `Enabled` or
  `Disabled`.
- **Wrong / Right:** `<ui-row heading="Sync" value="On"><ui-toggle slot="trailing">` / `<ui-row heading="Sync"><ui-toggle slot="trailing" label="Sync">`

### CTRL-09 A switch acts at once
- **Rule:** Flipping a switch takes effect immediately, unless the row is inside a sheet or form that the person
  commits with Done or Save.
- **Why (iPadOS):** Toggles; Settings.
- **Check:** `interact`.
- **Wrong / Right:** a Settings-style screen with a `Save` button for its switches / switches that apply when flipped.

### CTRL-10 A toggle button changes its fill
- **Rule:** An on/off button shows its state by its fill (the kit's `pressed`), not by colour of the glyph alone,
  and exposes `aria-pressed`.
- **Why (iPadOS):** Toggles.
- **Check:** `auto`: an element with `aria-pressed` whose background does not change between pressed and not pressed.
- **Wrong / Right:** a filter glyph that turns from gray to blue / `ui-button toggle pressed` with its tinted fill.

### CTRL-11 No square checkboxes
- **Rule:** iPadOS has no square checkbox: mark done or chosen with `ui-check` (round) or with a list's selection; an
  agreement is a switch row or a button.
- **Why (iPadOS):** Toggles.
- **Check:** `auto`: a visible `input[type=checkbox]` or `input[type=radio]` outside kit shadow roots.
- **Wrong / Right:** `<input type="checkbox"> I agree` / a `ui-row` `I Agree` with a `ui-toggle`, or an `Agree`
  button.

## Segmented controls and pickers

### CTRL-12 Two to five segments
- **Rule:** A `ui-segmented` has two to five segments of roughly equal content, all text or all glyphs.
- **Why (iPadOS):** Segmented controls.
- **Check:** `auto`: a `ui-segmented` or `[role=radiogroup]` segmented control with fewer than 2 or more than 5
  options.
- **Wrong / Right:** seven segments for days of the week / a `ui-picker`.

### CTRL-13 Segments select, they don't act
- **Rule:** A segmented control always shows which segment is chosen; it never runs commands (use buttons for that).
- **Why (iPadOS):** Segmented controls.
- **Check:** `interact`.
- **Wrong / Right:** segments `Start | Stop | Restart` / buttons for those.

### CTRL-14 Segment labels are short nouns
- **Rule:** Segment labels are nouns or noun phrases in title case, short enough to fit without truncation at the
  window's narrowest; the control needs no label in front of it.
- **Why (iPadOS):** Segmented controls.
- **Check:** `look` (a label cut off is TYPE-26).
- **Wrong / Right:** `Show only running ones` / `Running`

### CTRL-15 One of many is a picker
- **Rule:** Choosing one value from more than five, or from a set that changes, is `ui-picker`: `variant="bare"` as
  a row's trailing value in a form, the boxed field elsewhere; never a native `select`.
- **Why (iPadOS):** Pop-up buttons; Pickers.
- **Check:** `auto`: a visible `select` outside kit shadow roots.
- **Wrong / Right:** `<select>` with eight restart policies / `<ui-picker variant="bare" options=…>` in a row.

### CTRL-16 A picker always shows a choice
- **Rule:** A picker starts on a sensible default (the most likely option) and shows the current choice; its options
  are in a predictable order (alphabetical, numeric, or by frequency, stated).
- **Why (iPadOS):** Pop-up buttons; Pickers.
- **Check:** `auto`: a `ui-picker` that is not `required` whose `value` matches no option.
- **Wrong / Right:** a picker showing nothing until opened / `Restart: Unless Stopped`.

### CTRL-17 Pop-up for values, pull-down for actions
- **Rule:** A menu of mutually exclusive values that updates its button is a pop-up (`ui-picker`); a menu of commands
  is a pull-down whose button keeps its own glyph or label.
- **Why (iPadOS):** Pop-up buttons; Pull-down buttons.
- **Check:** `interact`.
- **Wrong / Right:** a `ui-picker` with `Delete`, `Rename`, `Duplicate` / a More pull-down with them.

### CTRL-18 A pull-down earns its menu
- **Rule:** A pull-down holds at least three items related to its button (Add, Sort, More); one or two actions are
  buttons. The view's primary actions are never only inside it.
- **Why (iPadOS):** Pull-down buttons.
- **Check:** `interact`.
- **Wrong / Right:** a More menu whose only item is `Refresh` / a `Refresh` glyph button.

## Sliders, steppers, disclosure, pages

### CTRL-19 Sliders run minimum to maximum, leading to trailing
- **Rule:** A slider's minimum is at the leading end (bottom when vertical) and the filled part of the track is the
  tint; end glyphs (`IconSlider`) show what small and large mean.
- **Why (iPadOS):** Sliders.
- **Check:** `interact`.
- **Wrong / Right:** a "quality" slider with best on the left / worst left, best right.

### CTRL-20 A stepper sits by the value it changes
- **Rule:** A stepper is right next to the visible value it changes, and a wide range also gets a field.
- **Why (iPadOS):** Steppers.
- **Check:** `look`.
- **Wrong / Right:** a lone `– +` at the end of a row with the number in the subtitle / `Replicas  3  – +`.

### CTRL-21 Disclosure says what it reveals
- **Rule:** A disclosure control (a chevron that turns down when open) is labelled with what it shows (`Advanced
  Options`) and sits right above that content; one disclosure button per view at most.
- **Why (iPadOS):** Disclosure controls.
- **Check:** `look`, `interact`.
- **Wrong / Right:** a bare `▸` / `▸ Advanced Options`.

### CTRL-22 Page dots only for flat pages
- **Rule:** A page control is for an ordered set of peer pages (ten or fewer), centred at the bottom; never for
  hierarchy or more pages than can be counted.
- **Why (iPadOS):** Page controls.
- **Check:** `look`.
- **Wrong / Right:** dots for 24 log files / a list.

## Labels, links, meters, glyphs

### CTRL-23 Read-only text is a label
- **Rule:** A value people read but cannot change is text (a row `value`, a label, selectable where useful), not a
  read-only or disabled field.
- **Why (iPadOS):** Labels.
- **Check:** `auto`: a visible `ui-text-field[readonly]`, `ui-text-area[readonly]` or `input[readonly]` in an app.
- **Wrong / Right:** `<ui-text-field readonly value="10.0.0.4">` / `<ui-row heading="Address" value="10.0.0.4">`
  with `user-select: text`.

### CTRL-24 Links go somewhere, buttons do something
- **Rule:** Something that opens a URL or another place is an `a[href]` in `--link`; something that runs an action is
  a button, even when it looks like text.
- **Why (iPadOS):** Buttons; Writing.
- **Check:** `auto`: an `a` without `href`, or a `[role=link]` that is not an `a[href]`; `source` for buttons that set
  `location`.
- **Wrong / Right:** `<a onclick="restart()">Restart</a>` / `<ui-button variant="plain">Restart</ui-button>`

### CTRL-25 A meter has a name and a number
- **Rule:** A `ui-meter` has a `label` (what is measured) and its value is also shown as text beside it.
- **Why (iPadOS):** Gauges.
- **Check:** `auto`: a `ui-meter` with a `value` and no `label`; `look` for the visible number.
- **Wrong / Right:** a bare bar at 61% / `Memory 9.8 GB of 16 GB` above the meter.

### CTRL-26 Glyphs come from the icon set
- **Rule:** Interface glyphs are `ui-icon` (`Icon` in the shell), by name; no emoji, icon fonts, bitmaps or inline SVG
  of an app's own as interface icons.
- **Why (iPadOS):** Icons; SF Symbols.
- **Check:** `auto`: an `img`, a non-`ui-icon` `svg` or an emoji as the only content of a control, or as a row's
  leading glyph, in an app.
- **Wrong / Right:** `<ui-button>🗑</ui-button>` / `<ui-button square label="Delete"><ui-icon slot="icon" name="trash"></ui-icon></ui-button>`

### CTRL-27 Standard actions use standard glyphs
- **Rule:** An action in the table above uses its glyph; a glyph from the table never means something else.
- **Why (iPadOS):** Icons; Buttons.
- **Check:** `look`, `source`.
- **Wrong / Right:** `xmark` for Delete, `plus` for Expand / `trash` for Delete.

### CTRL-28 Glyph weight matches the text
- **Rule:** A glyph next to text matches its weight (`weight` 2 beside regular text, 2.5 beside semibold) and all
  glyphs in one bar or list share one size and weight.
- **Why (iPadOS):** Icons.
- **Check:** `auto`: `ui-icon`s in one bar or one list with different `size` or `weight`.
- **Wrong / Right:** a 1.5-weight glyph beside a 2.5-weight one in the same bar / both 2.
