# Forms

How a form takes input and says what is wrong with it. The model is iPadOS: prevent
the mistake, say it where it was made, and keep alerts for what the form could not
have known. Worked example: `apps/dns` (`RecordForm`, `SettingsForm`).

## Layout

### FORM-01 A form is a list of sections, a row per value
- **Rule:** A form is a `ui-list` of one or more `ui-section`s, a row per value: a label and a
  `ui-text-field variant="bare"`, or a `heading` with a `ui-toggle` in `slot="trailing"`.
- **Why (iPadOS):** Text fields; Lists and tables.
- **Check:** `source`: a field or switch outside a `ui-row`, or a row holding two values.
- **Wrong / Right:** `<label>Name <input></label><label>Port <input></label>` /
  `<ui-section><ui-row heading="Name"><ui-text-field variant="bare" slot="trailing">…`

### FORM-02 On the window's own background, the list is inset
- **Rule:** A form that sits on the window's plain background uses `variant="inset"`; on the grouped background,
  the default `insetGrouped`.
- **Why (iPadOS):** Color.
- **Check:** `auto`: COLOR-10 for grouped; a `ui-list[variant=inset]` whose nearest opaque background is a grouped
  token.
- **Wrong / Right:** `insetGrouped` rows on a white pane / `variant="inset"` there.

### FORM-03 The footer is the form's one voice
- **Rule:** The section's `footer` is where the form speaks: a note on what the rows are for, or what is wrong with
  them. Nothing else under or between the rows says it.
- **Why (iPadOS):** Lists and tables.
- **Check:** `source`: hint or error text in a separate element next to a form section.
- **Wrong / Right:** `<p class="hint">` under the list / `<ui-section footer="Used by every container on this host.">`

### FORM-04 The commit button comes last
- **Rule:** The button that commits (`Add`, `Save`) sits under the section, at the end. In a sheet the commit is the
  bar's trailing item instead and Cancel the leading one (NAV-11).
- **Why (iPadOS):** Sheets; Buttons.
- **Check:** `look`.
- **Wrong / Right:** `Save` above the fields / `Save` under the last section.

### FORM-05 The label leads, the value trails
- **Rule:** In a form row the label is the row's title at the leading edge and the field, picker or value fills the
  trailing side on the same line; a field never sits above its label as in a web form.
- **Why (iPadOS):** Text fields; Apple's Settings and Contacts.
- **Check:** `look`; `auto`: a `ui-text-field` in a row whose top is below the bottom of the row's title.
- **Wrong / Right:** a label on one line and a full-width boxed field under it / `Port   8080`.

### FORM-06 Related fields share a section
- **Rule:** Group fields by what they describe (`Server`, `Authentication`) with a section header; split a form of
  more than about seven rows into sections; a field for more than one line of text is a `ui-text-area`.
- **Why (iPadOS):** Text fields; Layout.
- **Check:** `look`.
- **Wrong / Right:** twelve fields in one section / three sections of four.

## Prevent before reporting

### FORM-07 Offer a choice instead of text
- **Rule:** Wherever the values are known, offer a choice instead of a field: a toggle, a segmented control, a picker,
  a list.
- **Why (iPadOS):** Entering data; Text fields.
- **Check:** `look`.
- **Wrong / Right:** a text field for the protocol (`tcp`/`udp`) / `ui-segmented` `TCP | UDP`.

### FORM-08 Fields start with sensible defaults
- **Rule:** Give fields sensible defaults and fill what the system already knows; do not ask for it.
- **Why (iPadOS):** Entering data.
- **Check:** `look`.
- **Wrong / Right:** an empty TTL field / `TTL 3600`.

### FORM-09 The right keyboard and limits
- **Rule:** Give each field the type and keyboard hints for its content: `type` (`email`, `url`, `tel`, `number`,
  `search`, `password`), `inputmode` (`numeric`, `decimal`), `maxlength`, `min`/`max`/`step`, `enterkeyhint`.
- **Why (iPadOS):** Text fields; Virtual keyboards.
- **Check:** `auto`: a field whose label matches `/e-?mail/i` without `type=email`; `/\b(url|website|link)\b/i` without
  `type=url`; `/\b(port|count|number|pid|ttl|size|replicas|priority)\b/i` without `type=number` or `inputmode`
  `numeric`/`decimal`.
- **Wrong / Right:** `<ui-text-field label="Port">` / `<ui-text-field label="Port" inputmode="numeric" maxlength="5">`

### FORM-10 Identifiers are not capitalised or corrected
- **Rule:** Fields for identifiers (host names, user names, paths, URLs, e-mail, tokens, codes) set
  `autocapitalize="off"` so the keyboard does not change what is typed.
- **Why (iPadOS):** Virtual keyboards; Entering data.
- **Check:** `auto`: a field with `type` `email` or `url`, or a label matching
  `/\b(host|user ?name|path|domain|token|key|id|image|tag)\b/i`, whose `autocapitalize` is not `off` or `none`.
- **Wrong / Right:** `<ui-text-field label="Host">` / `<ui-text-field label="Host" autocapitalize="off">`

### FORM-11 Secrets use secure entry
- **Rule:** A password, token, passphrase or key is `type="password"` and is never prefilled with the stored secret
  (show that one exists in the footer instead).
- **Why (iPadOS):** Text fields; Entering data.
- **Check:** `auto`: a field whose label matches `/\b(password|passphrase|secret|token|api key|private key)\b/i` and
  whose `type` is not `password`; a `type=password` field with a non-empty value when its view opens.
- **Wrong / Right:** `<ui-text-field label="Token" value="ghp_…">` / `<ui-text-field label="Token" type="password">`

### FORM-12 Sign-in fields say what they are
- **Rule:** User name, e-mail and password fields set `autocomplete` (`username`, `email`, `current-password`,
  `new-password`) so the browser's password manager can fill them.
- **Why (iPadOS):** Entering data.
- **Check:** `auto`: a `type=password` field with no `autocomplete`.
- **Wrong / Right:** `<ui-text-field type="password">` / `<ui-text-field type="password" autocomplete="current-password">`

### FORM-13 Clearing a field is one tap
- **Rule:** A single-line field whose text people replace rather than edit (search, a URL, a name filter) shows a
  clear button at its trailing end while it has text; `SearchField` does. Escape clears a search field.
- **Why (iPadOS):** Text fields; Search fields.
- **Check:** `interact`.
- **Wrong / Right:** a search field cleared only by holding Backspace / a field with the `xmark.circle.fill` button.

### FORM-14 Paste and drop always work
- **Rule:** Fields accept pasted and dropped text; an app never blocks paste or strips it silently (normalise it
  visibly instead, such as trimming spaces).
- **Why (iPadOS):** Entering data.
- **Check:** `source`: a `paste` or `drop` listener on a field that calls `preventDefault()`.
- **Wrong / Right:** a token field that refuses paste / one that accepts it and trims the newline.

### FORM-15 Disabled until there is something to send
- **Rule:** The commit button is `disabled` until there is something to send and nothing wrong with it.
- **Why (iPadOS):** Entering data.
- **Check:** `interact`, `auto`: with a required field empty, the commit button is not `disabled`.
- **Wrong / Right:** an enabled `Add` that then reports "Name is required" / `Add` disabled until the name is typed.

### FORM-16 An empty required field is not an error
- **Rule:** An empty required field only disables the commit button; it is not an error to report.
- **Why (iPadOS):** Entering data.
- **Check:** `interact`: no red text or `footer-tone="error"` while a required field is merely empty.
- **Wrong / Right:** "Name can't be empty" in red on a fresh form / nothing until something is wrong.

## Saying what is wrong

### FORM-17 Validation is client state that mirrors the backend
- **Rule:** Validate in the client state (a `.svelte.ts` class exposing a problem per field), with pure functions that
  mirror the backend's checks. The backend validates again and has the last word.
- **Why (iPadOS):** Entering data.
- **Check:** `source`.
- **Wrong / Right:** validation inline in a component's markup / a `RecordFormState` class with `problems`.

### FORM-18 The field shows it is wrong
- **Rule:** Mark the field with `invalid` (red text in a row, a red outline for a boxed field).
- **Why (iPadOS):** Text fields.
- **Check:** `auto`: a section with `footer-tone="error"` and no row control with `invalid` / `aria-invalid="true"`.
- **Wrong / Right:** an error footer with every field looking fine / the faulty field in red.

### FORM-19 The sentence goes in the section's footer
- **Rule:** Put the sentence in the section's footer with `footer-tone="error"`.
- **Why (iPadOS):** Writing.
- **Check:** `source`; `auto`: a field with `invalid` whose section's `footer-tone` is not `error`.
- **Wrong / Right:** a toast "Port is invalid" / `footer="Use a port from 1 to 65535." footer-tone="error"`.

### FORM-20 One problem at a time, top to bottom
- **Rule:** The footer says one sentence at a time: the first problem, top to bottom.
- **Why (iPadOS):** Writing.
- **Check:** `interact`.
- **Wrong / Right:** three errors joined in one footer / the first one; the next after it is fixed.

### FORM-21 The field describes its problem to a screen reader
- **Rule:** Set the field's `description` to the same sentence, for a screen reader.
- **Why (iPadOS):** Accessibility.
- **Check:** `auto`: a field with `invalid` whose `aria-description` is empty or differs from its section's footer.
- **Wrong / Right:** `invalid` with no `description` / `invalid description="Use a port from 1 to 65535."`

### FORM-22 Speak after leaving, then follow each keystroke
- **Rule:** A field speaks once the person has left it (`focusout`), then follows every keystroke until it is right.
  Do not flag half-typed input. Input that can be judged per character (digits only, a port) may speak as it is typed.
  A new user name or password says what it still lacks while it is typed, before the person leaves the field.
- **Why (iPadOS):** Text fields ("when entering an email address, it's best to validate when people switch to another
  field; when creating a user name or password, validation needs to happen before people switch to another field").
- **Check:** `interact`.
- **Wrong / Right:** "Not an IP address" after typing `1` / after leaving the field with `192.168.1`.

### FORM-23 Say the rule and show a right value
- **Rule:** Say which rule was broken and show a right value: "Not an IP address, like 192.168.1.10 or fd00::10.",
  not "Invalid input".
- **Why (iPadOS):** Writing.
- **Check:** `look`.
- **Wrong / Right:** `Invalid input.` / `Not an IP address, like 192.168.1.10 or fd00::10.`

### FORM-24 A refusal from the backend lands on the form
- **Rule:** What only the backend can know (the name is taken) comes back as a refusal (`ctx:fail`, code -32010).
  Show it in the same footer, and clear it when the person types again.
- **Why (iPadOS):** Writing.
- **Check:** `interact`.
- **Wrong / Right:** a banner "Name taken" / the footer says it until the name changes.

## What is not the form's to say

### FORM-25 A banner is for the window, not for input
- **Rule:** A window-level `ui-banner` is for failures that are not about what was typed: the host is away, Docker
  refused. Never use it for input.
- **Why (iPadOS):** Feedback; Alerts.
- **Check:** `auto`: a `ui-banner` that appears as the direct result of submitting a form whose fields are invalid.
- **Wrong / Right:** a banner "Port must be a number" / the footer says it.

### FORM-26 No alert for input
- **Rule:** Never report input with a modal alert. An alert is for a failure discovered after the form was sent that
  needs acknowledging, and for confirming something destructive.
- **Why (iPadOS):** Alerts.
- **Check:** `interact`.
- **Wrong / Right:** an alert "Please enter a name" / the button stays disabled.

### FORM-27 No message outlives its cause
- **Rule:** A problem goes the moment the input is right, a refusal when the input changes, a failure when a call next
  succeeds.
- **Why (iPadOS):** Feedback.
- **Check:** `interact`.
- **Wrong / Right:** a red footer still showing after the value was fixed / gone on the keystroke that fixes it.

## Tests

### FORM-28 Unit-test the validators and the state
- **Rule:** Unit-test the validators and the state class: wrong input is not sent, the problem is exposed per field,
  a refusal lands on the form and a host failure on the window.
- **Why (iPadOS):** Entering data.
- **Check:** `source`.
- **Wrong / Right:** a form whose only test is a screenshot / tests for `problems`, refusal and failure.

### FORM-29 Stories and journeys assert what the person sees
- **Rule:** In a story or journey assert the section's `footer` and `footerTone`, the field's `invalid`, and the
  button's `disabled`; and that no banner appeared.
- **Why (iPadOS):** Entering data.
- **Check:** `source`.
- **Wrong / Right:** a journey that only checks the request failed / one that checks the footer, `invalid` and
  `disabled`.
