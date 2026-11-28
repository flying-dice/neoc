# Lists

Most of an iPadOS app is lists. In vos a list is `ui-list` with `ui-section`s of `ui-row`s (an app), or
`ListSection` with `ListRow`s (the shell). They draw the row height, insets, inset hairlines, header and footer type,
selection, keyboard movement and list semantics. An app sets properties; it does not restyle rows.

## Styles

| Variant | Use | Background behind it |
| --- | --- | --- |
| `insetGrouped` (default) | settings, forms, the attributes of one item | grouped (`--bg-grouped-elevated`) |
| `inset` | the same, on a plain pane | plain (`--bg-elevated`) |
| `grouped` | edge-to-edge groups in a narrow column | grouped |
| `plain` | a long list of like items (files, messages, processes, log entries) filling a column | plain |
| `sidebar` | navigation in a split view's first column | grouped |

## Row anatomy

`[selection check] [icon tile or leading glyph] [title / subtitle] … [value] [one trailing control | chevron]`

| Part | Property | Rule |
| --- | --- | --- |
| Title | `heading` | Body, `--label`, one line (TYPE-09) |
| Subtitle | `subheading` | Subheadline, `--label-secondary` (TYPE-10) |
| Value | `value` | Body, `--label-secondary` (TYPE-11) |
| Icon tile | `icon`, `icon-color` | 29 pt tile, a palette token name, never hex (COLOR-01) |
| Chevron | `chevron` | navigation only (LIST-04) |
| Control | `slot="trailing"` | one: a switch, a picker, a small button, a value pair |
| Action | `action` / `destructive` | tint / red title, no chevron |

---

### LIST-01 Lists are kit lists
- **Rule:** Rows are `ui-row` in a `ui-list` (the shell's `ListRow` in a `ListSection`); never a hand-made row or a
  `ul` styled as a list.
- **Why (iPadOS):** Lists and tables.
- **Check:** `auto`: an element with role `listitem`, `option` or `row`, or an `li` with a visible background or
  border, outside `ui-row` and `ListRow`, in an app.
- **Wrong / Right:** `<div class="row" role="listitem">` / `<ui-row heading="…">`

### LIST-02 The list style fits the content
- **Rule:** Settings, forms and an item's attributes are `insetGrouped` (or `inset`); a long list of like items in a
  column is `plain`; navigation in a split view is `sidebar`.
- **Why (iPadOS):** Lists and tables.
- **Check:** `look`.
- **Wrong / Right:** 400 log entries in inset-grouped blocks / a `plain` list.

### LIST-03 Use the row's properties
- **Rule:** When the row has a property for a part (`heading`, `subheading`, `value`, `icon`, `chevron`, `action`,
  `destructive`), use it; slot custom content only for what the row cannot show.
- **Why (iPadOS):** Lists and tables.
- **Check:** `source`: a `ui-row` whose default slot holds only one or two text spans, or whose trailing slot holds
  only secondary text.
- **Wrong / Right:** `<ui-row><span>Wi-Fi</span><span slot="trailing">Home</span></ui-row>` /
  `<ui-row heading="Wi-Fi" value="Home">`

### LIST-04 A chevron means it navigates
- **Rule:** `chevron` appears only on a `tappable` row that shows another view; never on a row that toggles, runs an
  action, opens a menu or alert, or is not tappable.
- **Why (iPadOS):** Lists and tables.
- **Check:** `auto`: a `ui-row[chevron]` that is not `tappable`, or is `action` or `destructive`, or holds a control in
  its trailing slot.
- **Wrong / Right:** `<ui-row heading="Sign Out" tappable chevron>` / `<ui-row heading="Sign Out" tappable destructive>`

### LIST-05 Rows that drill in show it
- **Rule:** In an inset-grouped or grouped list, a row that pushes a view has a chevron. In a sidebar, or a column
  whose choice shows in the next column, rows have no chevron: the selection highlight says it.
- **Why (iPadOS):** Lists and tables; Split views.
- **Check:** `interact`: tap each tappable row and compare with its chevron.
- **Wrong / Right:** a Settings-style row `General` that pushes but has no chevron / with `chevron`.

### LIST-06 An action row is tinted, not chevroned
- **Rule:** A row that performs an action (`Add Account…`, `Restart`) is `tappable` with `action` (tint title), or
  `destructive` (red) when it destroys; it has no value and no chevron.
- **Why (iPadOS):** Lists and tables; Buttons.
- **Check:** `source`; `auto`: a tappable row whose heading matches
  `/^(Add|New|Create|Restart|Start|Stop|Sign|Log|Connect|Disconnect|Reset|Export|Import|Run|Check|Install)\b/` and that
  has neither `action`, `destructive` nor `chevron`, flagged for review.
- **Wrong / Right:** `<ui-row heading="Add Host" tappable>` in black / `<ui-row heading="Add Host…" tappable action>`

### LIST-07 One control at a row's end
- **Rule:** A row's trailing slot holds at most one control (a switch, a picker, one small button) or one value pair;
  more actions go in the row's context menu or its detail.
- **Why (iPadOS):** Lists and tables.
- **Check:** `auto`: a `ui-row` whose trailing slot holds more than one interactive element.
- **Wrong / Right:** `Start`, `Stop` and `Delete` buttons in each row / a context menu with them.

### LIST-08 The info button only informs
- **Rule:** An `info.circle` button in a row shows more about that row (a popover), never navigation; a row that
  drills in uses the chevron.
- **Why (iPadOS):** Lists and tables.
- **Check:** `interact`.
- **Wrong / Right:** an info button that pushes the edit screen / an info button that shows a popover of details.

### LIST-09 A row shows a title, not the content
- **Rule:** Row text is short; long content (a message body, a log line's payload) is a one- or two-line preview, and
  the whole thing opens in a detail view.
- **Why (iPadOS):** Lists and tables.
- **Check:** `look`.
- **Wrong / Right:** a row that grows to 20 lines of JSON / a row with the first line and a chevron.

### LIST-10 Headers name groups, footers explain them
- **Rule:** A section header is a short noun naming the group, and is left out when one section fills the screen; a
  footer explains the rows above it. Neither holds a control.
- **Why (iPadOS):** Lists and tables.
- **Check:** `look`.
- **Wrong / Right:** a header `Below you can configure the network` / header `Network`, footer `Applies to new
  containers.`

### LIST-11 The kit draws the separators
- **Rule:** Rows are divided only by the kit's inset hairline (starting at the text, none after the last row, none in a
  sidebar); an app adds no borders, rules or gaps between rows.
- **Why (iPadOS):** Lists and tables; Color.
- **Check:** `auto`: a `ui-row` (or an app wrapper around it) with a visible border, `box-shadow` line or margin.
- **Wrong / Right:** `ui-row { border-bottom: 1px solid var(--separator) }` / nothing.

### LIST-12 Navigation highlights, choice checks
- **Rule:** A list whose choice shows in the next column keeps the chosen row highlighted (`selection="single"`);
  a list of options to pick one from marks the chosen row with a trailing tint checkmark and no fill.
- **Why (iPadOS):** Lists and tables.
- **Check:** `look`, `interact`.
- **Wrong / Right:** a "Restart Policy" option list with a filled blue row / the chosen option with a checkmark.

### LIST-13 Multiple selection is a mode
- **Rule:** Selecting several rows happens in a select mode (`Select` in the bar, `selection="multi"`, round checks at
  the leading edge), with the actions for the selection in the bar; leaving the mode clears it.
- **Why (iPadOS):** Lists and tables.
- **Check:** `interact`.
- **Wrong / Right:** a check circle permanently beside every row / checks that appear after `Select`.

### LIST-14 Every swipe action is also in the menu
- **Rule:** An action offered by swiping a row is also in the row's context menu (and in edit mode, if it deletes),
  so a pointer, keyboard or screen reader can reach it.
- **Why (iPadOS):** Accessibility; Context menus.
- **Check:** `interact`.
- **Wrong / Right:** delete only by swiping / swipe or `Delete` in the context menu.

### LIST-15 Delete is the trailing, red swipe action
- **Rule:** In a trailing swipe, `Delete` is red and the outermost action; non-destructive actions (Pin, Mark as Read)
  go in the leading swipe.
- **Why (iPadOS):** Lists and tables; Apple's Mail and Notes.
- **Check:** `interact`.
- **Wrong / Right:** a blue `Delete` beside a red `Archive` / a red `Delete` at the far trailing end.

### LIST-16 An empty list says so
- **Rule:** A list with nothing in it is replaced by a `ui-empty-state` (title, message, the action to add one), never
  left as headers over no rows or a blank pane.
- **Why (iPadOS):** Writing; Lists and tables.
- **Check:** `auto`: a visible `ui-list` with no visible `ui-row` and no visible `ui-empty-state` in the same column.
- **Wrong / Right:** a `Containers` header over nothing / `No Containers`, `Containers you create appear here.`,
  `Add Container`.

### LIST-17 Long lists scroll, they don't page
- **Rule:** A long list loads more as it scrolls (with a spinner row at the end while loading); there are no page
  numbers or Next Page buttons.
- **Why (iPadOS):** Lists and tables; Scroll views.
- **Check:** `look`.
- **Wrong / Right:** `‹ Page 2 of 9 ›` under a list / continuous scrolling.
