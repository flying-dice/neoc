# Help, onboarding and first run

The best iPadOS app needs no help: its screens say what they are and its controls what they do. When an app does
need to teach, it does so briefly, in place, and once. There is no manual.

---

## Launch and first run

### HELP-01 The first screen is the app
- **Rule:** An app opens straight to its first useful screen; no splash screen, logo, version banner or "About" card
  first, and the placeholder shown while it starts looks like that screen (same background and bars, no text).
- **Why (iPadOS):** Launching.
- **Check:** `look`.
- **Wrong / Right:** a two-second logo animation / the list of notes, filling in.

### HELP-02 It comes back where it was
- **Rule:** Reopening an app restores what the person had: the selected item, the column in front, scroll position and
  any draft, as far as the app can know them.
- **Why (iPadOS):** Launching; Multitasking.
- **Check:** `interact`: select an item, scroll, close and reopen the window.
- **Wrong / Right:** every open lands on the first sidebar row / the last container viewed is selected again.

### HELP-03 It works before it is configured
- **Rule:** Sensible defaults let a person use the app at once; setup that is not essential waits until the feature
  that needs it is used.
- **Why (iPadOS):** Onboarding; Settings.
- **Check:** `interact` on a fresh profile.
- **Wrong / Right:** a five-screen preferences wizard on first open / defaults, and a prompt when a registry is first
  needed.

### HELP-04 Ask for access when it is needed, and say why
- **Rule:** A permission or credential is requested when the person first uses the feature that needs it, with one
  sentence on why; not up front for everything.
- **Why (iPadOS):** Onboarding.
- **Check:** `interact`.
- **Wrong / Right:** asking for notification permission on first open / when the person turns on `Notify When Done`.

## Onboarding

### HELP-05 Onboarding only if it is needed, and brief
- **Rule:** Show onboarding only for what cannot be learned by using the app: at most three short screens, each one
  idea, preferably letting the person try the action.
- **Why (iPadOS):** Onboarding.
- **Check:** `look`.
- **Wrong / Right:** eight slides of features / one screen that lets you add your first host.

### HELP-06 Onboarding can be skipped and is shown once
- **Rule:** Every onboarding screen has a way past it; once finished or skipped it never appears again by itself, and
  it can be found later (from the app's settings or help).
- **Why (iPadOS):** Onboarding.
- **Check:** `interact`: skip, reopen the app.
- **Wrong / Right:** a tour on every launch / once, then under `Show Tour` in settings.

### HELP-07 Teach the app, not the system
- **Rule:** Onboarding and help explain this app's own tasks and terms, never how standard controls, windows or
  gestures work.
- **Why (iPadOS):** Onboarding; Offering help.
- **Check:** `look`.
- **Wrong / Right:** "Use the switch to turn things on and off" / "Turn on Auto Restart to bring a stopped container
  back."

## Help in context

### HELP-08 Help sits next to what it explains
- **Rule:** Explanations live where the question arises: a section footer under the setting, a line in an empty
  state, a tip by the control; never a separate help page or a "?" that opens a document.
- **Why (iPadOS):** Offering help; Writing.
- **Check:** `look`.
- **Wrong / Right:** a `Help` button opening a long page about the Port setting / the footer
  `The port other machines connect to.`

### HELP-09 A tip is one or two sentences, once
- **Rule:** A tip describes one simple feature (three steps or fewer) in one or two action-oriented sentences, appears
  near that feature only for people who have not used it, can be dismissed, and is never promotional; at most one tip
  per view at a time.
- **Why (iPadOS):** Offering help.
- **Check:** `interact`.
- **Wrong / Right:** a tip about five features on every launch / "Drag a container onto a network to connect it."

### HELP-10 Glyph-only controls name themselves on hover
- **Rule:** A control shown only as a glyph has a tooltip (`title`, or the kit's equivalent) naming what it does,
  matching its accessible name, which appears when a pointer rests on it.
- **Why (iPadOS):** Offering help; Pointing devices.
- **Check:** `auto`: a button whose only content is an icon and that has no `title` (on it or its host element).
- **Wrong / Right:** a bare `arrow.clockwise` button / with `title="Restart"`.

### HELP-11 A tooltip adds, it never carries the essentials
- **Rule:** A tooltip is a short phrase in sentence case without a period that says what the control does; it does
  not repeat a visible label, and nothing a person needs exists only in a tooltip (touch has no hover).
- **Why (iPadOS):** Offering help; Accessibility.
- **Check:** `look`; `auto`: an element whose `title` equals its visible text; `source`: read every `title` and
  tooltip text, and find what only it says (a limit, a reason, a format).
- **Wrong / Right:** `title="Save"` on a button labelled `Save`; a limit explained only in a tooltip / a footer for
  the limit, no tooltip on the labelled button.

## Settings

### HELP-12 Options live where they act
- **Rule:** Options about the current view (sort, filter, show or hide columns, layout) are in that view (a bar
  pull-down, a segmented control); the app's settings hold only rarely changed, general options.
- **Why (iPadOS):** Settings.
- **Check:** `look`.
- **Wrong / Right:** "Sort processes by" in the app's settings / a `Sort By` pull-down above the list.

### HELP-13 No copies of system settings
- **Rule:** An app never offers its own version of a system-wide setting (theme, text size, accent, Reduce Motion,
  Reduce Transparency); it follows the ones in vos Settings and links there if needed.
- **Why (iPadOS):** Settings; Dark Mode.
- **Check:** `source`, `look`.
- **Wrong / Right:** a `Font Size` slider in an app / the app's text follows Settings › Text Size.
