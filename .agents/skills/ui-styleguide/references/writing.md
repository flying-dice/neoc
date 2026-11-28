# Writing

The words on a screen are part of its design: an iPadOS screen reads short, plain and the same everywhere. These
rules cover every string a person sees or hears, including accessible names.

## Capitalisation per element

Two styles, chosen per element type and never mixed within one:

- **Title case**: capitalise every word except articles (a, an, the), coordinating conjunctions (and, but, or, nor,
  for, so, yet) and prepositions of four letters or fewer (as, at, by, for, from, in, into, of, off, on, onto, out,
  over, per, to, up, via, with), unless the word is first or last. A hyphenated compound capitalises both parts
  ("Auto-Join"), and a product or term keeps its own case ("iCloud", "Wi-Fi", "vos").
- **Sentence case**: capitalise the first word and proper nouns only.

| Element | Style | Ending punctuation | Example |
| --- | --- | --- | --- |
| Window, navigation bar, sheet and screen titles | Title | none | `Recently Deleted` |
| Sidebar items, tab names | Title | none | `Smart Lists` |
| Buttons (all styles, bars, alerts, rows with `action`) | Title | none (`…` if more input follows) | `Add Account…` |
| Menu items (context, pull-down, pop-up) | Title | none (`…` if more input follows) | `Move to Folder…` |
| Segment labels, picker options | Title | none | `Last 7 Days` |
| Row titles and field labels in a form, toggle labels | Title | none | `Start Automatically` |
| Section headers (shown in caps by the style) | Title | none | `Allowed Hosts` |
| Table column headings | Title | none | `Last Modified` |
| Empty-state titles | Title | none | `No Notes` |
| Alert title that is a fragment | Title | none; a question keeps `?` | `Delete “Shopping”?` |
| Alert title that is a full sentence | Sentence | period or `?` | `The host stopped responding.` |
| Alert messages, section footers, empty-state messages | Sentence | full sentences end with a period | `Its todos are deleted with it.` |
| Banner and toast text, banner headings | Sentence | headings none, sentences a period | `Could not save` |
| Error text under a field or in a footer | Sentence | period | `Use a port from 1 to 65535.` |
| Placeholders | Sentence | none | `name@example.com` |
| Tooltips (`title`), help text beside a control | Sentence | none for a fragment | `Resize the window` |
| Notification titles / bodies | Title / Sentence | title none; body full sentences with a period | `Backup Finished` / `3 files were copied.` |

This follows Apple's iPadOS 16 and 17 apps and the kit (`Add Account…`, `Delete Work?`). The current HIG asks for
sentence case on alert buttons; the pre-Liquid-Glass apps vos copies use title case, so vos does.

---

## Capitalisation

### WRITE-01 Titles are title case
- **Rule:** Window, bar, sheet, screen, sidebar and tab titles are title case with no ending punctuation.
- **Why (iPadOS):** Writing; Toolbars.
- **Check:** `auto`: a bar or window title (`NavigationBar` title, `[role=dialog]` heading, sidebar row title) in which
  a word of four or more letters outside the lowercase list starts with a lowercase letter.
- **Wrong / Right:** `Recently deleted` / `Recently Deleted`

### WRITE-02 Buttons and menu items are title case
- **Rule:** Every button label and menu item is title case: `Add to Cart`, `Don’t Allow`, `Move to Trash`.
- **Why (iPadOS):** Buttons; Menus.
- **Check:** `auto`: the accessible name of a `button`, `menuitem` or `ui-button` with a word of four or more letters
  outside the lowercase list that starts lowercase.
- **Wrong / Right:** `Add a variable` / `Add Variable`

### WRITE-03 Row titles and field labels are title case
- **Rule:** The label of a form row, a settings row or a toggle is title case: `Start Automatically`, `Working
  Directory`, `Runs As`.
- **Why (iPadOS):** Writing; Settings.
- **Check:** `auto`: a `ui-row` `heading` that is a label (the row holds a control, a value or a chevron) with a word
  of four or more letters outside the lowercase list starting lowercase; `look` for content rows (a note's title is
  the person's own text).
- **Wrong / Right:** `<ui-row heading="Start automatically">` / `<ui-row heading="Start Automatically">`

### WRITE-04 Section headers and column headings are title case
- **Rule:** Write section headers and table column headings in title case, as nouns or noun phrases with no
  punctuation, even though grouped headers show in caps.
- **Why (iPadOS):** Lists and tables.
- **Check:** `source`.
- **Wrong / Right:** `header="Allowed hosts:"` / `header="Allowed Hosts"`

### WRITE-05 Messages are sentence case
- **Rule:** Footers, alert messages, banner and toast text, error text, help text and empty-state messages are
  sentence case.
- **Why (iPadOS):** Alerts; Notifications; Offering help.
- **Check:** `auto`: a `ui-section` footer, `ui-alert` message, `ui-banner` text or `ui-empty-state` message in which
  three or more words after the first start with a capital and are not proper nouns or acronyms (flag for review).
- **Wrong / Right:** `Changes Take Effect After A Restart.` / `Changes take effect after a restart.`

### WRITE-06 An alert title's case follows its grammar
- **Rule:** An alert title that is a fragment is title case without a period (a question fragment keeps its `?`); one
  that is a complete sentence is sentence case with ending punctuation. Prefer the fragment.
- **Why (iPadOS):** Alerts.
- **Check:** `look`, `source`.
- **Wrong / Right:** `Delete this note?` / `Delete Note?`

## Tone and brevity

### WRITE-07 Say it in the fewest plain words
- **Rule:** Cut every word that does not change the meaning; prefer short everyday words; no exclamation marks.
- **Why (iPadOS):** Writing.
- **Check:** `look`.
- **Wrong / Right:** `Please note that in order to proceed you will need to sign in!` / `Sign in to continue.`

### WRITE-08 Neutral and calm, never cute
- **Rule:** No jokes, interjections or exclamations in system text (`Oops!`, `Uh-oh`, `Whoa`, `Yay`), and no
  emoji in labels or messages.
- **Why (iPadOS):** Writing; Alerts.
- **Check:** `auto`: text matching `/\b(oops|uh-?oh|whoops|yikes|yay|woohoo)\b/i` or an emoji code point in an
  interface string (outside user content).
- **Wrong / Right:** `Oops! Something went wrong 😬` / `Could not load the containers.`

### WRITE-09 No "we", sparing "your"
- **Rule:** Never speak as "we" or "I". Leave out "my" and "your" when the context already says whose it is; if an
  app uses one, it uses only that one.
- **Why (iPadOS):** Writing.
- **Check:** `auto`: interface text matching `/\b(we|we're|we've|our|us)\b/i`; `look` for "your".
- **Wrong / Right:** `We couldn’t reach your server.` / `Could not reach the server.`

### WRITE-10 Device-neutral verbs
- **Rule:** Say what the person does with neutral verbs (choose, select, open, drag, press a key); never "click",
  "tap" or "right-click" in copy, since vos runs under a mouse, a pen and a finger.
- **Why (iPadOS):** Writing ("describe gestures correctly on each device"). The HIG says "tap" on iPad; vos departs
  because one screen of vos meets every kind of pointer.
- **Check:** `auto`: interface text matching `/\b(click|tap|right-click|double-click)\w*\b/i` (outside user content).
- **Wrong / Right:** `Click + to add a host.` / `Choose Add Host to add one.`

### WRITE-11 Link text says where it goes
- **Rule:** A link's text names its destination or effect; never "click here", "here", "this" or "link" alone.
- **Why (iPadOS):** Writing; Accessibility.
- **Check:** `auto`: an `a[href]` whose accessible name matches `/^(click )?here$|^this$|^link$|^more$/i`.
- **Wrong / Right:** `To learn about volumes, click <a>here</a>.` / `<a>About Volumes</a>`

## Buttons and actions

### WRITE-12 A button says what it does, with a verb
- **Rule:** A button label is a verb or verb phrase naming the result (`Delete`, `Add Host`, `Restart`), one to three
  words. A noun alone is fine only for a navigation button (`Settings`).
- **Why (iPadOS):** Writing; Buttons.
- **Check:** `look`; `auto`: a button name longer than three words.
- **Wrong / Right:** `Let’s do it!` / `Send`

### WRITE-13 No OK where a verb fits
- **Rule:** `OK` is only the single button of a purely informational alert. A confirmation uses the action's verb.
  `Yes` and `No` are never button labels. Set `ui-alert`'s `action-label` every time: its default is `OK`.
- **Why (iPadOS):** Alerts.
- **Check:** `auto`: a `ui-alert` with no `action-label` or `action-label="OK"`, an `Alert` with a non-cancel `OK`
  action beside another action, or any button named `Yes` or `No`.
- **Wrong / Right:** `<ui-alert heading="Delete Work?">` (shows OK) / `<ui-alert heading="Delete Work?" action-label="Delete">`

### WRITE-14 Cancel is always "Cancel"
- **Rule:** The button that backs out without changing anything is labelled exactly `Cancel`, never `Close`, `No`,
  `Back` or `Never Mind`.
- **Why (iPadOS):** Alerts; Sheets.
- **Check:** `auto`: a button with role `cancel` (shell `Alert`), `ui-alert` `cancel-label`, or a sheet's leading bar
  button whose label is not `Cancel`.
- **Wrong / Right:** `cancel-label="No, keep it"` / `cancel-label` left at `Cancel`

### WRITE-15 Done ends, Cancel abandons
- **Rule:** `Done` closes a view whose changes are already kept (or confirms them); `Cancel` closes and throws them
  away. A multi-step flow starts with `Continue` or `Next` (pick one per app) and ends with `Done`.
- **Why (iPadOS):** Writing; Sheets.
- **Check:** `interact`.
- **Wrong / Right:** a sheet whose `Close` discards edits / `Cancel` discards, `Done` keeps.

### WRITE-16 Delete destroys, Remove takes out
- **Rule:** `Delete` is for what stops existing; `Remove` is for taking an item out of a list, folder or group while it
  still exists elsewhere. Use the same verb for the same act in every app.
- **Why (iPadOS):** Writing; Apple's apps.
- **Check:** `interact`, `source`.
- **Wrong / Right:** `Delete from Favorites` / `Remove from Favorites`

### WRITE-17 An ellipsis means more input follows
- **Rule:** A button or menu item that needs more input before it acts (opens a sheet, a picker, a file chooser) ends
  in the ellipsis character `…`: `Rename…`, `Add Account…`. One that acts at once, or only shows something, has none.
- **Why (iPadOS):** Menus; Buttons.
- **Check:** `interact` (does choosing it act at once?); the character itself is WRITE-19.
- **Wrong / Right:** `Delete…` that deletes at once / `Export…` that opens a sheet; `Delete` that deletes.

## Punctuation, numbers, dates

### WRITE-18 No ending punctuation on labels
- **Rule:** Titles, buttons, menu items, row labels, headers, segment labels and placeholders have no period, colon
  or exclamation mark at the end. Footers and messages that are full sentences end with a period.
- **Why (iPadOS):** Alerts; Menus; Segmented controls.
- **Check:** `auto`: the accessible name of a button, menu item, tab, segment, heading or `ui-row` heading ends in
  `.`, `:` or `!` (an ellipsis is allowed).
- **Wrong / Right:** `<ui-row heading="Port:">` / `<ui-row heading="Port">`

### WRITE-19 Typographic punctuation
- **Rule:** Use the ellipsis character `…`, curly quotes `“ ”` and apostrophes `’`, and an en dash `–` for ranges;
  never `...`, straight `"` or `'` in interface text.
- **Why (iPadOS):** Menus; Apple's apps.
- **Check:** `auto`: a rendered text node containing `...`, `"` or `'`, outside `code`, `pre`, `kbd`, `samp`, `input`,
  `textarea`, `[contenteditable]`, `[role=log]` and terminal output.
- **Wrong / Right:** `Don't show again...` / `Don’t Show Again…`

### WRITE-20 Quote a name the person gave
- **Rule:** In a title or message, put a person-named item in curly quotes so it reads as a name: `Delete “Shopping”?`.
- **Why (iPadOS):** Alerts; Apple's apps.
- **Check:** `look`.
- **Wrong / Right:** `Delete Shopping?` / `Delete “Shopping”?`

### WRITE-21 Numbers are numerals, formatted for the locale
- **Rule:** Counts and quantities are numerals (`3 items`), grouped and formatted with `Intl.NumberFormat` for the
  person's locale; plurals agree (`1 item`, `2 items`), never `item(s)`.
- **Why (iPadOS):** Writing.
- **Check:** `auto`: text matching `/\(s\)/` or a run of five or more digits with no grouping in a count label;
  `source` for `Intl`.
- **Wrong / Right:** `1 items`, `12000 file(s)` / `1 item`, `12,000 files`

### WRITE-22 Units are standard and spaced
- **Rule:** A unit follows its number after a space (`16 GB`, `200 ms`, `3 days`), in standard symbols (`KB`, `MB`,
  `GB` in decimal units, as Apple counts storage); `%` and `°` attach (`61%`). One app never mixes `MB` and `MiB`.
- **Why (iPadOS):** Writing; Apple's Settings › Storage.
- **Check:** `auto`: text matching `/\d(KB|MB|GB|TB|ms|s)\b/` (no space) or `/\d\s%/`.
- **Wrong / Right:** `612MB`, `61 %` / `612 MB`, `61%`

### WRITE-23 Dates read like a person would say them
- **Rule:** Dates and times use `Intl.DateTimeFormat` for the locale; recent ones are relative (`Today`, `Yesterday`,
  `5 min ago`) where Apple's apps do so, older ones a short date. ISO stamps and epoch numbers appear only in
  technical detail.
- **Why (iPadOS):** Writing; Apple's Mail and Files.
- **Check:** `auto`: text matching `/\d{4}-\d{2}-\d{2}T\d{2}:/` or a 10–13 digit epoch outside `code`, `pre`,
  `[role=log]`.
- **Wrong / Right:** `2026-11-28T09:41:00Z` / `Today at 9:41`

## Errors

### WRITE-24 An error says what happened and what to do
- **Rule:** An error message names what failed, in the person's terms, and the next step (retry, change a value,
  check a setting). One or two sentences.
- **Why (iPadOS):** Writing; Alerts.
- **Check:** `look`.
- **Wrong / Right:** `Request failed.` / `Could not reach the host. Check that it is running, then try again.`

### WRITE-25 Errors don't blame
- **Rule:** Say what to do, not what the person did wrong: no "invalid", "illegal", "you failed", "wrong", "bad".
- **Why (iPadOS):** Writing.
- **Check:** `auto`: error text (`footer-tone="error"`, `[role=alert]`, `ui-banner tone="error"`) matching
  `/\b(invalid|illegal|wrong|bad|you failed|not allowed)\b/i`, flagged for review.
- **Wrong / Right:** `Invalid port.` / `Use a port from 1 to 65535.`

### WRITE-26 Language includes everyone
- **Rule:** Write gender-neutral, plain copy: "they" for an unknown person, no idioms or culture-bound jokes, nothing
  that assumes ability ("see", "hear") where a neutral verb works.
- **Why (iPadOS):** Writing.
- **Check:** `look`; `auto`: interface text matching `/\b(he or she|his or her|guys|whitelist|blacklist|master|slave)\b/i`.
- **Wrong / Right:** `Ask the admin to update his settings.` / `Ask an administrator to update the settings.`

### WRITE-27 No developer text in the interface
- **Rule:** No raw error strings, codes, stack traces, JSON, `null`, `undefined`, `NaN`, `[object Object]`, errno
  names (`ECONNREFUSED`) or RPC codes in interface text. A technical detail, when useful, sits behind a disclosure
  or in a copyable detail, after the plain message.
- **Why (iPadOS):** Writing; Alerts.
- **Check:** `auto`: rendered text outside `code`, `pre`, `samp`, `[role=log]` and terminal output matching
  `/\b(undefined|null|NaN|\[object Object\]|E[A-Z]{4,}|Exception|Traceback|-32\d{3})\b/` or starting `Error:`.
- **Wrong / Right:** `Error: connect ECONNREFUSED 127.0.0.1:2375` / `Docker is not running.`

### WRITE-28 No jargon the person did not bring
- **Rule:** Name things the way the person knows them; an app's own domain terms (container, port, DNS record) are
  fine in that app, implementation terms (RPC, handler, payload, mutex, context) are not.
- **Why (iPadOS):** Writing.
- **Check:** `look`.
- **Wrong / Right:** `The handler rejected the payload.` / `The record could not be saved.`

## Alerts, fields, empty states, settings

### WRITE-29 An alert title says the situation
- **Rule:** An alert title is specific and at most two lines: what is about to happen or what happened. Never
  `Error`, `Warning`, `Are you sure?` or `Confirm`.
- **Why (iPadOS):** Alerts.
- **Check:** `auto`: an alert heading matching `/^(error|warning|alert|confirm|are you sure\??|attention)$/i`.
- **Wrong / Right:** `Are you sure?` / `Delete “Shopping”?`

### WRITE-30 An alert message adds something or is left out
- **Rule:** The message is optional; when present it is one or two complete sentences that add a consequence or a
  reason, never a restatement of the title or an explanation of the buttons.
- **Why (iPadOS):** Alerts.
- **Check:** `look`.
- **Wrong / Right:** title `Delete Work?`, message `Do you want to delete Work? Choose Delete to delete it.` /
  message `Its todos are deleted with it.`

### WRITE-31 A placeholder is a hint, not the label
- **Rule:** A placeholder shows a format or example (`name@example.com`, `8080`); the field's label says what it is,
  in the row's title or an accessible name. A placeholder never holds an instruction the person needs while typing.
- **Why (iPadOS):** Text fields; Writing.
- **Check:** `auto`: a `ui-text-field`/`input` with a placeholder and no accessible name other than the placeholder.
- **Wrong / Right:** `<ui-text-field placeholder="Port">` alone / `<ui-row heading="Port"><ui-text-field
  variant="bare" label="Port" placeholder="8080">`

### WRITE-32 An empty state says what is missing and what to do
- **Rule:** The title names what is not there (`No Notes`, `No Results`); the message, in one sentence, says how to
  get some; the action button, if there is one, repeats the verb (`New Note`).
- **Why (iPadOS):** Writing.
- **Check:** `look`.
- **Wrong / Right:** `Nothing here!` / `No Notes`, `Notes you write appear here.`, `New Note`

### WRITE-33 A setting says what On does
- **Rule:** A toggle's label names the feature; if it needs explaining, the footer says what happens when it is on,
  and not what happens when it is off.
- **Why (iPadOS):** Writing; Settings.
- **Check:** `look`.
- **Wrong / Right:** `Enable or disable automatic restart` / `Restart Automatically`, footer `Restarts the service if
  it stops.`

### WRITE-34 One term for one thing
- **Rule:** Each concept has one name across the shell and every app (a "window" is never also a "pane" or "view"),
  and controls are named in copy exactly as they are labelled.
- **Why (iPadOS):** Writing.
- **Check:** `source`, `look`.
- **Wrong / Right:** a button `Remove`, its alert `Delete this item?` / button `Delete`, alert `Delete “x”?`
