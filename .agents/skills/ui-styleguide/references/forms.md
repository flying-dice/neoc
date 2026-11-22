# Forms

How a form takes input and says what is wrong with it. The model is iPadOS: prevent
the mistake, say it where it was made, and keep alerts for what the form could not
have known. Worked example: `apps/dns` (`RecordForm`, `SettingsForm`).

## Layout

- A form is a `ui-list` of one or more `ui-section`s, a row per value: a label and a
  `ui-text-field variant="bare"`, or a `heading` with a `ui-toggle` in `slot="trailing"`.
  Use `variant="inset"` on the window's own background.
- The section's `footer` is the form's one voice: a note on what the rows are for, or
  what is wrong with them.
- The button that commits (`Add`, `Save`) sits under the section, at the end.

## Prevent before reporting

- Offer a choice instead of text wherever the values are known: a toggle, a
  segmented control, a list. Give fields sensible defaults and the right keyboard
  (`inputmode`, `autocapitalize="off"`, `maxlength`).
- The commit button is `disabled` until there is something to send and nothing wrong
  with it. An empty required field disables the button; it is not an error to report.

## Saying what is wrong

- Validate in the client state (a `.svelte.ts` class exposing a problem per field),
  with pure functions that mirror the backend's checks. The backend validates again
  and has the last word.
- Mark the field (`invalid`: red text in a row) and put the sentence in the section's
  footer with `footer-tone="error"`. One sentence at a time: the first problem, top
  to bottom. Set the field's `description` to it too, for a screen reader.
- A field speaks once the person has left it (`focusout`), then follows every
  keystroke until it is right. Do not flag half-typed input. Input that can be judged
  per character (digits only, a port) may speak as it is typed.
- Say which rule was broken and show a right value: "Not an IP address, like
  192.168.1.10 or fd00::10.", not "Invalid input".
- What only the backend can know (the name is taken) comes back as a refusal
  (`ctx:fail`, code -32010). Show it in the same footer, and clear it when the person
  types again.

## What is not the form's to say

- A window-level `ui-banner` is for failures that are not about what was typed: the
  host is away, Docker refused. Never use it for input.
- Never report input with a modal alert. An alert is for a failure discovered after
  the form was sent that needs acknowledging, and for confirming something destructive.
- No message outlives its cause: a problem goes the moment the input is right, a
  refusal when the input changes, a failure when a call next succeeds.

## Tests

- Unit-test the validators and the state class: wrong input is not sent, the problem
  is exposed per field, a refusal lands on the form and a host failure on the window.
- In a story or journey assert the section's `footer` and `footerTone`, the field's
  `invalid`, and the button's `disabled`; and that no banner appeared.
