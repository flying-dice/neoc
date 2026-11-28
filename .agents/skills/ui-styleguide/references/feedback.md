# Feedback

Feedback says what is happening, what happened, and what to do about it, at a volume that matches its weight.
vos's pieces: inline text in rows and footers, `ui-spinner` (`ActivityIndicator`), `ui-meter`, `ui-banner` in the
window, `ui-empty-state`, the shell's `Toast` and `NotificationBanner`, and `ui-alert` for the rare interruption.

## Where a message goes

| What | Where | Element |
| --- | --- | --- |
| Status of an item (running, syncing, 3 unread) | on the item: its row value or subtitle | `ui-row` |
| Something wrong with what was typed | the section footer | `ui-section footer-tone="error"` (forms.md) |
| A failure not about input (host away, refused) | the top of the window or its column | `ui-banner tone="error"` |
| A decision that cannot wait, or an irreversible risk | an alert | `ui-alert` |
| A significant success, an Undo offer | a toast | `Toast` |
| Something the person should know while away | a notification | `NotificationBanner` |
| Nothing to show | the empty state | `ui-empty-state` |
| Work in progress | a spinner or a meter where the result will appear | `ui-spinner`, `ui-meter` |

---

## Status and progress

### FEED-01 Feedback matches its weight
- **Rule:** Routine success is silent (the change is visible), status is shown passively in place, a failure that
  blocks work is a banner, and only a decision that cannot wait interrupts with an alert.
- **Why (iPadOS):** Feedback; Alerts.
- **Check:** `look`.
- **Wrong / Right:** a toast for every saved setting / the switch flips and stays.

### FEED-02 Status sits on what it describes
- **Rule:** The state of an item (running, failed, syncing, a count) is shown on the item itself (its row's value,
  subtitle or a glyph with text), not in a separate panel or popup.
- **Why (iPadOS):** Feedback.
- **Check:** `look`.
- **Wrong / Right:** a status window listing container states / each container row's subtitle says `Running`.

### FEED-03 Known amounts get a determinate indicator
- **Rule:** When the size of the work is known (bytes, items, steps), show a determinate `ui-meter` or progress bar
  with the amount in text; use an indeterminate `ui-spinner` only when it is not.
- **Why (iPadOS):** Progress indicators.
- **Check:** `interact`.
- **Wrong / Right:** a spinner for a 120 MB image pull with known size / a meter and `42 MB of 120 MB`.

### FEED-04 Spinners are the kit's
- **Rule:** An indeterminate indicator is `ui-spinner` (the shell's `ActivityIndicator`); no CSS-animated rings,
  GIFs or dots of an app's own.
- **Why (iPadOS):** Progress indicators.
- **Check:** `source` (a hand-made loader; its endless animation is also caught by MOTION-08).
- **Wrong / Right:** `.loader { animation: spin 1s infinite }` / `<ui-spinner label="Loading containers">`

### FEED-05 Progress moves honestly
- **Rule:** A determinate indicator advances at an even, truthful pace and never sits still; if the work stalls, the
  text says so and what can be done.
- **Why (iPadOS):** Progress indicators.
- **Check:** `interact`.
- **Wrong / Right:** 90% in one second, then a minute at 90% / an even pace, and `Waiting for the registry…` if it
  stalls.

### FEED-06 One indicator shape per task
- **Rule:** A task keeps the indicator it started with (spinner or bar), except that an indeterminate bar may become
  determinate; a spinner never turns into a bar.
- **Why (iPadOS):** Progress indicators.
- **Check:** `interact`.
- **Wrong / Right:** a spinner replaced mid-task by a progress bar / a meter from the start.

### FEED-07 Progress says what it is doing
- **Rule:** The text with an indicator names the work specifically (`Pulling nginx:1.27`), never a bare `Loading…`;
  a `ui-spinner`'s `label` names what loads, not its default `Loading`.
- **Why (iPadOS):** Progress indicators.
- **Check:** `auto`: a visible `ui-spinner` whose `label` is `Loading` or empty; a visible text `Loading…` alone.
- **Wrong / Right:** `<ui-spinner>` + `Loading…` / `<ui-spinner label="Loading logs">` + `Loading logs for web…`

### FEED-08 Long work can be stopped
- **Rule:** A long operation that can be stopped without harm has a `Cancel` (and `Pause` if cancelling loses work);
  cancelling that loses progress asks first.
- **Why (iPadOS):** Progress indicators.
- **Check:** `interact`.
- **Wrong / Right:** an unstoppable five-minute build / `Cancel` beside its progress.

## Loading

### FEED-09 Never a blank screen
- **Rule:** A view shows its bar, title and layout at once, with cached content or placeholders, and fills in as data
  arrives; a centred spinner alone only when there is nothing else to show.
- **Why (iPadOS):** Loading.
- **Check:** `auto`: 200ms after a view is shown, its content root contains no text, no `ui-spinner` and no
  `ui-empty-state`.
- **Wrong / Right:** a white window for two seconds / the list's bar and title with a spinner in its body.

### FEED-10 Content arrives without a jump
- **Rule:** Placeholders take the size of what replaces them, so nothing the person is looking at or about to press
  moves when data arrives.
- **Why (iPadOS):** Loading.
- **Check:** `auto`: a layout shift (PerformanceObserver `layout-shift`) above 0.1 in the first 3 seconds of a view
  without input.
- **Wrong / Right:** a button that drops 200px when the list loads above it / a reserved list area.

### FEED-11 The rest stays usable while loading
- **Rule:** Loading one part never blocks the window: other rows, the bar and navigation keep working; no modal
  "please wait" over the whole window.
- **Why (iPadOS):** Loading.
- **Check:** `interact`.
- **Wrong / Right:** a dimmed window with a spinner while logs load / logs load in their pane.

### FEED-12 Content refreshes itself
- **Rule:** Data that changes is kept current automatically; a manual Reload is an extra, never the only way to see
  new data.
- **Why (iPadOS):** Progress indicators.
- **Check:** `interact`.
- **Wrong / Right:** a process list that only updates on `Refresh` / one that updates live.

### FEED-13 Empty and loading are different
- **Rule:** The empty state shows only once loading has finished with no data; while loading, a spinner or
  placeholders show, never `No Items` first.
- **Why (iPadOS):** Loading; Writing.
- **Check:** `interact` (throttle the network): `ui-empty-state` never appears before the first response.
- **Wrong / Right:** `No Containers` flashes, then the list / spinner, then the list.

## Errors, banners, toasts

### FEED-14 An error appears where it happened
- **Rule:** Report a failure as close as possible to what failed: the field (footer), the row (its subtitle in red with
  a glyph), the column or window (a banner); an alert only when the person must decide now.
- **Why (iPadOS):** Feedback; Writing.
- **Check:** `look`.
- **Wrong / Right:** an alert because one row failed to restart / that row says `Failed to restart` with a retry.

### FEED-15 A command that cannot run says why
- **Rule:** A control that is unavailable is dimmed (not hidden, outside context menus), and the reason is visible
  nearby (a footer, a subtitle, the empty state); never only in a tooltip, which touch cannot reach (HELP-11).
- **Why (iPadOS):** Feedback ("Show people when a command can't be carried out and help them understand why");
  Offering help (a tooltip appears only when a pointer rests on a control).
- **Check:** `look`.
- **Wrong / Right:** a dimmed `Start` whose reason is only in its tooltip / footer `Start Docker to run containers.`

### FEED-16 A banner offers the fix
- **Rule:** A `ui-banner` for a failure offers the fix as its action when there is one (`Retry`, `Open Settings`), as
  a `ui-button slot="action" variant="plain" size="small"`. (When it leaves is FORM-27; that it never times out,
  A11Y-20.)
- **Why (iPadOS):** Feedback.
- **Check:** `look`, `interact`.
- **Wrong / Right:** `Could not reach the host.` with no way to retry / the same with `Retry`.

### FEED-17 One banner per window
- **Rule:** A window shows at most one banner at a time, the most important; its tone matches (`error` only for
  failures).
- **Why (iPadOS):** Feedback; Alerts.
- **Check:** `auto`: more than one visible `ui-banner` in one window.
- **Wrong / Right:** three stacked banners / one.

### FEED-18 Toasts confirm, they don't warn
- **Rule:** A toast confirms a significant completed action or offers Undo; it is never an error, a warning or the
  only place something needed is said. (How long it stays is A11Y-20.)
- **Why (iPadOS):** Feedback; Alerts.
- **Check:** `look`, `interact`.
- **Wrong / Right:** a toast "Could not connect" that vanishes / a banner for that; a toast `Image deleted · Undo`.

### FEED-19 Undoable deletes offer Undo, not a warning
- **Rule:** An undoable destructive action runs at once and offers Undo where the person is looking (a toast action,
  and ⌘Z); it does not ask first.
- **Why (iPadOS):** Feedback; Alerts; Undo and redo.
- **Check:** `interact`.
- **Wrong / Right:** "Delete this note?" / the note goes, `Note deleted · Undo`.

### FEED-20 Feedback is announced
- **Rule:** Every status, error or progress message that appears is also announced: in a `status` region (polite) or,
  for an error, an `alert` region (the kit's banner and footer do this).
- **Why (iPadOS):** Feedback.
- **Check:** `auto`: text that appears after load in a banner, footer error or toast with no `role=status`,
  `role=alert` or `aria-live` on it or an ancestor.
- **Wrong / Right:** a `div` that turns red with `Failed` / `ui-banner tone="error"`.

## Notifications and badges

### FEED-21 Notify only while away
- **Rule:** A notification is for timely news when the app is not in front; when it is in front, update the view in
  place instead. Never notify about an error (use the app's banner or an alert), never twice for one thing.
- **Why (iPadOS):** Notifications.
- **Check:** `interact`.
- **Wrong / Right:** a notification "Container stopped" while the app is showing that container / its row updates.

### FEED-22 A notification is short and private
- **Rule:** A notification's title is short and title case without punctuation, its body full sentences, it does not
  repeat the app's name (the shell shows it), tells nobody what to do later, and holds no secrets.
- **Why (iPadOS):** Notifications.
- **Check:** `look`.
- **Wrong / Right:** `Containers: Your container web has stopped. Open Containers to restart it.` /
  title `web Stopped`, body `It exited with code 137.`

### FEED-23 Badges count unread notifications only
- **Rule:** A badge shows only the number of unread notifications, is updated as soon as they are read, and nothing
  else imitates a badge.
- **Why (iPadOS):** Notifications.
- **Check:** `look`.
- **Wrong / Right:** a red capsule with the CPU percentage / a plain trailing value.
