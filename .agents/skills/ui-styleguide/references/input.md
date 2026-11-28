# Input

People use vos with a mouse or trackpad, a keyboard, a finger or a pen, and switch between them freely; every
interaction works with each. This file covers the pointer, the keyboard (shortcuts, Tab, Return, Escape), drag and
drop, scrolling, windows and multitasking, and undo.

vos departs from iPadOS on one point: on iPad without Full Keyboard Access, Tab moves only between text fields; vos
is a web desktop held to WCAG, so Tab reaches every control (one stop per group, arrows within it), as the kit does.

## Standard shortcuts

On Apple platforms the modifier is ⌘; elsewhere it is Ctrl (the shell's `isApplePlatform`). Shown in the order ⌃ ⌥ ⇧ ⌘.

| Shortcut | Action |
| --- | --- |
| ⌘Z / ⇧⌘Z | Undo / Redo |
| ⌘X, ⌘C, ⌘V, ⌘A | Cut, Copy, Paste, Select All |
| ⌘F | Find or search in the current view |
| ⌘N | New item or document |
| ⌘W | Close the window |
| ⌘, | The app's settings |
| ⌘. and Esc | Cancel |
| ⌘K | System search (the shell's own) |

---

## Pointer

### INPUT-01 Controls show the pointer, text shows the I-beam
- **Rule:** Enabled controls show `cursor: pointer` (as the kit's do), editable text `text`, disabled controls
  `default`; resize and drag handles use the matching resize or grab cursor. No decorative custom cursors.
- **Why (iPadOS):** Pointing devices.
- **Check:** `auto`: an enabled interactive element (not a text input) whose computed `cursor` is `auto` or `default`;
  a `cursor: url(…)`.
- **Wrong / Right:** a hand-made clickable row with the arrow cursor / `ui-row tappable` (pointer).

### INPUT-02 Hover highlights, it doesn't decorate
- **Rule:** On pointer hover, small controls (bar glyphs, menu items) get a subtle rounded fill and large elements a
  tint; nothing scales up in a list, gains a shadow without scaling, or animates for show.
- **Why (iPadOS):** Pointing devices.
- **Check:** `auto`: an element whose computed `transform` changes to a scale above 1 on `:hover` inside a list or bar;
  `interact` (hover each kind of control) for the rest.
- **Wrong / Right:** `ui-row:hover { transform: scale(1.03); box-shadow: … }` / a `--fill-quaternary` hover fill.

### INPUT-03 Bar targets touch each other
- **Rule:** Adjacent items in a bar have contiguous hit areas, so the pointer never falls into a dead gap between
  them.
- **Why (iPadOS):** Pointing devices.
- **Check:** `auto`: in one bar, a horizontal gap of more than 1px between the hit boxes of two neighbouring items
  that are less than 44px apart.
- **Wrong / Right:** 34 px glyph buttons with 10 px dead space between them / 44 px hit areas that meet.

## Keyboard

### INPUT-04 Shortcuts are declared and shown
- **Rule:** A command with a keyboard shortcut declares it on its control (`aria-keyshortcuts`) and names it in the
  control's tooltip; the shortcut uses the platform's modifier.
- **Why (iPadOS):** Keyboards; Accessibility.
- **Check:** `source`: a `keydown` handler for a modified key whose command's control has no `aria-keyshortcuts`.
- **Wrong / Right:** ⌘R restarts with nothing saying so / the Restart button has `aria-keyshortcuts="Meta+R"` and
  `title="Restart (⌘R)"`.

### INPUT-05 Standard shortcuts keep their meaning
- **Rule:** The shortcuts in the table above do what the table says in every app, and an app never takes them (or the
  shell's ⌘K) for something else.
- **Why (iPadOS):** Keyboards.
- **Check:** `interact`, `source`.
- **Wrong / Right:** ⌘N opens Settings / ⌘N makes a new item, ⌘, opens Settings.

### INPUT-06 Few custom shortcuts, made with ⌘
- **Rule:** Custom shortcuts exist only for frequent commands, use ⌘ as the main modifier (⇧ second, ⌥ rarely, never
  ⌃), and never add a modifier to a standard shortcut for an unrelated command.
- **Why (iPadOS):** Keyboards.
- **Check:** `source`.
- **Wrong / Right:** ⌃⌥K for a rare export, ⇧⌘Z for "Zoom" / ⌘E for Export… if it is frequent.

### INPUT-07 One tab stop per group, arrows inside
- **Rule:** A list, segmented control, menu, radio group or grid is one tab stop; the arrow keys (and Home and End)
  move inside it, as the kit's elements do.
- **Why (iPadOS):** Keyboards; Accessibility.
- **Check:** `auto`: a `listbox`, `radiogroup`, `menu`, `grid`, `tablist` or `ui-list` with more than one descendant at
  `tabIndex` 0.
- **Wrong / Right:** Tab stops on each of 40 rows / one stop, then the arrows.

### INPUT-08 Return does the default thing
- **Rule:** Return in a single-line field submits its form (the kit's fields do); in a dialog it activates the
  focused or preferred button; in a text area it starts a new line.
- **Why (iPadOS):** Buttons; Alerts.
- **Check:** `interact`.
- **Wrong / Right:** Return in a form's last field does nothing / it submits.

### INPUT-09 Escape closes only the top layer
- **Rule:** Escape closes the topmost transient layer (a menu, a popover, an alert, a search query) and nothing behind
  it; with nothing open it does nothing destructive.
- **Why (iPadOS):** Keyboards.
- **Check:** `auto`: with a menu open over a dialog, one Escape leaves the dialog open.
- **Wrong / Right:** one Escape closes the menu and the sheet under it / only the menu.

### INPUT-10 Focus follows the view
- **Rule:** When the keyboard changes the view (a row opens its detail, a column comes to the front), focus moves into
  the new view (as `ui-split-view` does); it is never left on something that disappeared.
- **Why (iPadOS):** Keyboards; Split views.
- **Check:** `auto`: after pressing Enter on a row that changes the front column, `activeElement` is `body` or is not
  inside the visible column.
- **Wrong / Right:** focus stays on a hidden sidebar row / it moves to the detail column.

## Drag and drop

### INPUT-11 Drag where people expect to
- **Rule:** Items that can be moved or shared (files, list items, text) can be dragged to the places that accept them:
  other lists, other columns, other apps' windows, and the desktop where the shell supports it.
- **Why (iPadOS):** Drag and drop.
- **Check:** `interact`.
- **Wrong / Right:** a file list that cannot be dragged into a folder / drag to move.

### INPUT-12 Destinations say whether they accept
- **Rule:** While something is dragged over a place that accepts it, that place highlights (or shows an insertion
  line); a place that does not accept shows nothing; the highlight goes when the item leaves.
- **Why (iPadOS):** Drag and drop.
- **Check:** `interact`.
- **Wrong / Right:** every list lights up whatever is dragged / only valid targets do.

### INPUT-13 A failed drop goes back
- **Rule:** Dropping on an invalid place, or a drop that fails, animates the item back to where it came from (or away),
  and nothing changes.
- **Why (iPadOS):** Drag and drop.
- **Check:** `interact`.
- **Wrong / Right:** the item vanishes with no result / it returns to its place.

### INPUT-14 Move inside, copy across
- **Rule:** Dropping within the same container moves; into another container or app copies; holding ⌥ forces a copy.
- **Why (iPadOS):** Drag and drop.
- **Check:** `interact`.
- **Wrong / Right:** dragging a note to another folder copies it and dragging within a folder duplicates it / move,
  and copy with ⌥.

### INPUT-15 Dropped items stay selected, and a drop can be undone
- **Rule:** After a drop the dropped items are selected in their new place, and ⌘Z undoes the drop.
- **Why (iPadOS):** Drag and drop.
- **Check:** `interact`.
- **Wrong / Right:** items land unselected and the move is final / selected, undoable.

## Scrolling

### INPUT-16 No scroller inside a scroller on the same axis
- **Rule:** A scrolling region never sits inside another that scrolls the same way; a horizontal strip inside a
  vertical page is fine.
- **Why (iPadOS):** Scroll views.
- **Check:** `auto`: an element with `overflow-y: auto|scroll` and `scrollHeight > clientHeight` inside an ancestor that
  also scrolls vertically (and the same for x).
- **Wrong / Right:** a scrolling log box inside a scrolling detail pane / the log fills the pane and is the one scroller.

### INPUT-17 Scrolling is the system's
- **Rule:** Scrolling uses native overflow with the wheel, trackpad, keyboard (arrows, Page Up/Down, Space) and touch;
  no scroll hijacking, custom momentum or snapping that fights the gesture.
- **Why (iPadOS):** Scroll views.
- **Check:** `source`: a `wheel` listener that calls `preventDefault()` outside a canvas that zooms or pans.
- **Wrong / Right:** a list that scrolls one row per wheel notch / native scrolling.

## Windows and multitasking

### INPUT-18 Resizing keeps everything
- **Rule:** Resizing a window or moving it between layouts (floating, Split View, Slide Over, full screen) keeps the
  selection, scroll position, typed text and open column, and adapts live.
- **Why (iPadOS):** Multitasking; Windows.
- **Check:** `interact`: type in a field, then resize across the 700 px breakpoint and back.
- **Wrong / Right:** a half-typed form cleared on resize / it survives.

### INPUT-19 No window chrome of an app's own
- **Rule:** An app draws no title bar, close, minimise or traffic-light buttons; the shell's `Window` provides them.
- **Why (iPadOS):** Windows.
- **Check:** `look`, `source`.
- **Wrong / Right:** an `×` in the top corner of an app's page / the window's own "···" menu.

### INPUT-20 Items worth their own window can have one
- **Rule:** Where an item makes sense on its own (a document, a container's logs), its context menu offers `Open in New
  Window`. (That choosing a row never opens a window by itself is NAV-04.)
- **Why (iPadOS):** Windows.
- **Check:** `interact`.
- **Wrong / Right:** no way to see two logs side by side / `Open in New Window`.

### INPUT-21 Work pauses or carries on sensibly in the background
- **Rule:** When an app's window is hidden or minimised, media and anything that needs watching pause; work the person
  started (a pull, an upload) carries on and is waiting, finished, on return.
- **Why (iPadOS):** Multitasking.
- **Check:** `interact`.
- **Wrong / Right:** a download that stops when the window is minimised / it continues.

## Undo and redo

### INPUT-22 Edits can be undone, many times
- **Rule:** Every edit and every undoable deletion in an app supports ⌘Z and ⇧⌘Z, with as many levels as the session
  has steps.
- **Why (iPadOS):** Undo and redo.
- **Check:** `interact`.
- **Wrong / Right:** one level of undo, or none / every step since the window opened.

### INPUT-23 Undo shows what it undid
- **Rule:** After an undo or redo the affected item is brought into view and selected, so the person sees the change.
- **Why (iPadOS):** Undo and redo.
- **Check:** `interact`.
- **Wrong / Right:** undoing a delete far down the list with no visible change / the list scrolls to the restored row.

### INPUT-24 Undo says what it will undo
- **Rule:** Where undo is labelled (a menu item, a toast action), it names the action: `Undo Delete`, `Redo Rename`; a
  dedicated undo button is used only when needed, in the bar, with `arrow.uturn.backward` and `arrow.uturn.forward`.
- **Why (iPadOS):** Undo and redo.
- **Check:** `look`.
- **Wrong / Right:** a lone `Undo` with a custom glyph in the content / `Undo Delete` in the toast.
