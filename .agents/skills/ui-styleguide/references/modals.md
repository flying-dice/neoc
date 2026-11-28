# Modals

A modal view stops work in its window until it is answered: an alert, an action sheet, a sheet, a full-window cover.
A popover is lighter: it closes when the person looks elsewhere. vos has `ui-alert` (an app) and `Alert` (the shell);
the shell's `ContextMenu` and `MultitaskingMenu` are its popover-style menus. There is no sheet or popover element
yet: an app does not hand-roll one; it shows the task in the detail column or a new window until the kit has it.

| Need | Use |
| --- | --- |
| Something went wrong that needs acknowledging, or confirm a rare destructive act | alert (`ui-alert`) |
| Choices about an action the person just started (Save Draft, Delete Draft) | action sheet (on iPad, a popover from the button) |
| A short scoped task (New Container, Edit Record) | sheet (form sheet, centred) |
| A little information or a few controls about something on screen | popover |
| Immersive media, or a long multistep edit | full-window cover |
| Status that needs no answer | not a modal: inline text, `ui-banner`, a toast (feedback.md) |

---

## Modality in general

### MODAL-01 One modal at a time
- **Rule:** At most one sheet, popover or menu is open; a sheet never opens over a sheet, a popover never from a
  popover. Only an alert may appear over a sheet, and never two alerts.
- **Why (iPadOS):** Modality; Popovers; Sheets.
- **Check:** `auto`: more than one open element among `dialog[open]`, `[aria-modal=true]`, `[role=menu]` and popovers
  in one window, other than a single `alertdialog` above one `dialog`.
- **Wrong / Right:** a confirmation sheet stacked on an edit sheet / close the edit sheet, then confirm.

### MODAL-02 Modals are kit or shell elements
- **Rule:** An alert is `ui-alert` (`Alert` in the shell), never `window.alert`, `confirm` or `prompt`, and never a
  hand-made fixed overlay with a scrim.
- **Why (iPadOS):** Alerts; Modality.
- **Check:** `auto`: a call to `window.alert`, `window.confirm` or `window.prompt` (stub them in the test page); an
  app element with `position: fixed|absolute` covering 80% or more of its window with a background alpha of 0.2 or
  more, outside kit and shell components.
- **Wrong / Right:** `if (confirm('Delete?'))` / `<ui-alert heading="Delete “web”?" action-label="Delete" destructive>`

### MODAL-03 A modal dims what it blocks
- **Rule:** An alert or a sheet dims its window with `--dimming`; a popover and a menu do not dim.
- **Why (iPadOS):** Modality; Sheets; Popovers.
- **Check:** `auto`: an open modal dialog whose backdrop or scrim colour is not `--dimming`; a popover or menu
  shown with a scrim.
- **Wrong / Right:** a sheet over an undimmed window / the window under `rgb(0 0 0 / 0.4)`.

### MODAL-04 Focus stays inside and comes back
- **Rule:** While a modal is open, focus starts inside it, Tab cycles inside it and the window behind is inert;
  when it closes, focus returns to the control that opened it.
- **Why (iPadOS):** Modality; Accessibility.
- **Check:** `auto`: after opening, `activeElement` (through shadow roots) is inside the dialog; 20 presses of Tab
  never leave it; after closing, `activeElement` is the opener.
- **Wrong / Right:** Tab walks into the page behind an alert / it cycles between Cancel and Delete.

### MODAL-05 Escape means Cancel
- **Rule:** Escape answers a modal as Cancel would: an alert, a popover, a menu, and a sheet with no unsaved changes
  close; a sheet with changes asks first (MODAL-17).
- **Why (iPadOS):** Alerts.
- **Check:** `auto`: pressing Escape in an open alert, popover or menu closes it without firing its confirm action.
- **Wrong / Right:** Escape does nothing in a dialog / Escape closes it as Cancel.

### MODAL-06 A modal names its task
- **Rule:** A sheet or full-window modal has a title naming its task (`New Container`, `Edit Record`), so a person
  returning to it knows where they are.
- **Why (iPadOS):** Modality.
- **Check:** `look`; `auto`: a `[role=dialog]` with no accessible name.
- **Wrong / Right:** an untitled sheet of fields / `New Container`.

### MODAL-07 No hierarchy inside a modal
- **Rule:** A modal task is one view, or one linear sequence of steps; it never contains its own navigation tree or
  buttons that look like they dismiss it.
- **Why (iPadOS):** Modality.
- **Check:** `look`.
- **Wrong / Right:** a sheet with a sidebar of five sections / a two-step sheet with Back and Done.

## Alerts

### MODAL-08 Alerts are for what matters now
- **Rule:** Use an alert only for a problem that needs an answer, or to confirm an uncommon destructive act that cannot
  be undone; never for information alone, never at launch, never to confirm a routine delete that can be undone.
- **Why (iPadOS):** Alerts.
- **Check:** `look`, `interact`.
- **Wrong / Right:** "Are you sure you want to delete this note?" for an undoable delete / delete at once, offer Undo.

### MODAL-09 An alert title fits in two lines
- **Rule:** An alert's title wraps to at most two lines and its message is short enough that the alert never scrolls
  at the default Text Size.
- **Why (iPadOS):** Alerts.
- **Check:** `auto`: at `s` = 1, the alert title's height exceeds 2 × its line height, or the alert's text box has
  `scrollHeight > clientHeight`.
- **Wrong / Right:** a five-line title / `Delete “web”?` with a one-line message.

### MODAL-10 Two buttons side by side, three stacked, never more
- **Rule:** An alert has one to three buttons; two sit side by side, three stack vertically.
- **Why (iPadOS):** Alerts.
- **Check:** `auto`: an alert with more than three buttons, or three buttons in one row.
- **Wrong / Right:** an alert with `Save`, `Don’t Save`, `Save As…`, `Cancel` / an action sheet.

### MODAL-11 Cancel leads, the likely answer trails
- **Rule:** In a row, `Cancel` is the leading button and the most likely answer the trailing one; in a stack, the most
  likely answer is on top and `Cancel` at the bottom.
- **Why (iPadOS):** Alerts.
- **Check:** `auto`: in a two-button alert, the button named `Cancel` is not the leading one; in a stacked alert, it is
  not the last.
- **Wrong / Right:** `Delete | Cancel` / `Cancel | Delete`

### MODAL-12 A destructive alert has Cancel
- **Rule:** An alert whose action destroys has a `Cancel` button, its destructive button is red (`destructive`),
  and Cancel holds the focus so a stray Return deletes nothing.
- **Why (iPadOS):** Alerts.
- **Check:** `auto`: an alert with a destructive button and no `Cancel`; a destructive alert whose initial focus is the
  destructive button.
- **Wrong / Right:** a one-button `Delete` alert / `Cancel | Delete` (red), focus on Cancel.

### MODAL-13 The safe likely answer is the default
- **Rule:** In a non-destructive alert the most likely answer is bold and takes Return; an alert that only needs
  acknowledging (a failure the person must know about, MODAL-08) has one button, `OK`.
- **Why (iPadOS):** Alerts; Buttons.
- **Check:** `auto`: a non-destructive two-button alert whose bold, focused button is `Cancel`.
- **Wrong / Right:** `Cancel` bold in "Restart now?" / `Restart` bold.

## Action sheets

### MODAL-14 Choices about the person's own action are an action sheet
- **Rule:** When an action the person started needs a choice (discard or keep a draft), present an action sheet — on
  iPad a popover anchored to the button — not an alert and not a menu.
- **Why (iPadOS):** Action sheets.
- **Check:** `look`.
- **Wrong / Right:** an alert "Save changes?" with three buttons / `Delete Draft` and `Save Draft` in a popover from
  `Cancel`.

### MODAL-15 Destructive first, Cancel last
- **Rule:** In an action sheet the destructive choice is red and at the top; `Cancel` is at the bottom (left out in
  the popover form, where tapping outside cancels); it never scrolls. In a compact window (under 700px) there is no
  popover: the action sheet rises from the bottom of the window, with `Cancel`.
- **Why (iPadOS):** Action sheets ("Place the Cancel button at the bottom"; "Avoid letting an action sheet scroll");
  Popovers ("Avoid displaying popovers in compact views").
- **Check:** `look`.
- **Wrong / Right:** `Save Draft`, `Delete Draft` in black, `Cancel` first / `Delete Draft` red on top.

## Sheets

### MODAL-16 A sheet has Done and Cancel, or Back
- **Rule:** A sheet with a confirming button (`Done`, `Save`, `Add`) also has `Cancel` (or `Back` on a later step);
  never all three; `Back` never closes the sheet.
- **Why (iPadOS):** Sheets.
- **Check:** `auto`: a sheet's bar with `Done`/`Save`/`Add` and neither `Cancel` nor `Back`, or with all of `Cancel`,
  `Back` and `Done`.
- **Wrong / Right:** a sheet with only `Done` / `Cancel … Done`.

### MODAL-17 Closing with changes asks first
- **Rule:** Cancelling, pressing Escape or clicking outside a sheet that holds unsaved input asks what to do with it
  (an action sheet: `Discard Changes`, `Keep Editing`), instead of losing it.
- **Why (iPadOS):** Modality; Sheets.
- **Check:** `interact`.
- **Wrong / Right:** a half-filled form gone on Escape / `Discard Changes?`.

### MODAL-18 Sheets are for short tasks, centred
- **Rule:** A sheet holds one short task, centred on the dimmed window at the size of its content (a form sheet), not
  the whole window; it has no medium detent or grabber in a regular-width window (those are for a compact window,
  where a resizable sheet shows a grabber). A long or complex flow gets its own window or a full-window cover.
- **Why (iPadOS):** Sheets.
- **Check:** `look`.
- **Wrong / Right:** a ten-step setup in a sheet / a setup window.

## Popovers

### MODAL-19 A popover points at what opened it
- **Rule:** A popover appears next to the control that opened it, with its arrow pointing at it, without covering
  that control or what the person needs to see while using it.
- **Why (iPadOS):** Popovers.
- **Check:** `auto`: an open popover whose rect intersects the rect of its anchor.
- **Wrong / Right:** a popover centred on screen, over its button / one hanging from the button's edge.

### MODAL-20 A popover closes by itself and keeps the work
- **Rule:** A popover closes when the person clicks outside it or picks an item (unless several picks are allowed);
  whatever was changed in it is kept. Only an explicit `Cancel` discards.
- **Why (iPadOS):** Popovers.
- **Check:** `interact`.
- **Wrong / Right:** edits in a popover lost on an outside click / applied as made.

### MODAL-21 Popovers are small and only in regular width
- **Rule:** A popover is just large enough for its content (a few related controls or a short list); in a compact
  window the same content is a sheet.
- **Why (iPadOS):** Popovers.
- **Check:** `auto`: an open popover wider than 50% of its window or than 400px; a popover in a window narrower than
  700px.
- **Wrong / Right:** a 900px-wide settings popover / a sheet.

### MODAL-22 Never warn in a popover
- **Rule:** A warning or a confirmation of something risky is an alert, never a popover, which is easy to miss and to
  dismiss by accident.
- **Why (iPadOS):** Popovers.
- **Check:** `look`.
- **Wrong / Right:** a popover "This will erase the volume" / an alert.

## Full-window covers

### MODAL-23 A full-window cover is for immersion
- **Rule:** Cover the whole window only for viewing media or a long, focused edit; keep its essential controls
  reachable and give it a visible `Done` that the person chooses, never a timeout.
- **Why (iPadOS):** Modality; Going full screen.
- **Check:** `look`, `interact`.
- **Wrong / Right:** a settings form as a full-window cover / an image viewer with `Done`.
