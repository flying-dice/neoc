# Source: what to read in the code

The rules the screen cannot show: read the component's source for each. `lint-source.ts` points at many of them; it does not settle them.

## First: list what the pictures miss

Each picture is a story's last frame, so what a play function opened and closed again is in none of them. Before
looking, search the app's `src/` and list:
- every `ui-alert`, and every destructive action (`destructive`, delete and remove handlers): look.md section 7 and
  interact.md journey 6 decide whether each is the right shape and whether it can be undone;
- every message: `ui-banner`, toasts, `footer-tone`, `invalid`, `ui-empty-state`, and red or status text drawn by
  hand: feedback.md's table says where each belongs;
- every menu (`contextmenu`, `aria-haspopup`, a pull-down): what is in it, and which items of the same kind lack it;
- every `disabled`: where the person reads why (FEED-15);
- every `title` and every style that shows something on `:hover`:
  - [ ] **HELP-11** Read each tooltip: what it says that nothing visible says (a limit, a reason, a format) is a finding, since touch has no hover.
- every `keydown` and `aria-keyshortcuts`: the app's shortcuts (INPUT-04 to INPUT-06, below).

A state with no story (an alert open, a menu open, a form showing its error) goes under Not checked as a missing story.

## Then each component

Two causes of what the pictures show are only found here:
- Each `font: var(--type-…)` needs its `letter-spacing: var(--tracking-…)` beside it: the shorthand does not set it,
  so the text keeps the tracking it inherits (TYPE-25).
- `align-items: center` on a row of texts of different sizes or paddings is the usual reason a baseline steps
  (TYPE-50).

## Text: type ([typography.md](../../ui-styleguide/references/typography.md))

- [ ] **TYPE-10** A row subtitle is Subheadline, secondary
- [ ] **TYPE-11** A row's trailing value is Body, secondary
- [ ] **TYPE-12** A grouped section header is Footnote caps, secondary
- [ ] **TYPE-13** A sidebar section header is Headline
- [ ] **TYPE-14** A section footer is Footnote, secondary
- [ ] **TYPE-17** Buttons take the kit's type
- [ ] **TYPE-24** Monospace only for code and data
- [ ] **TYPE-31** What is cut can be read in full
- [ ] **TYPE-38** Caps are never typed
- [ ] **TYPE-44** Secondary text is secondary by colour, not opacity
- [ ] **TYPE-46** Text on a material uses label colours

## Text: words ([writing.md](../../ui-styleguide/references/writing.md))

- [ ] **WRITE-04** Section headers and column headings are title case
- [ ] **WRITE-06** An alert title's case follows its grammar
- [ ] **WRITE-16** Delete destroys, Remove takes out
- [ ] **WRITE-21** Numbers are numerals, formatted for the locale
- [ ] **WRITE-34** One term for one thing

## Colour ([color.md](../../ui-styleguide/references/color.md))

- [ ] **COLOR-01** No colour literals in app code
- [ ] **COLOR-07** The tint is the person's accent
- [ ] **COLOR-09** What destroys is red
- [ ] **COLOR-19** Both themes, from one declaration
- [ ] **COLOR-21** Nothing relies on Increase Contrast

## Layout ([layout.md](../../ui-styleguide/references/layout.md))

- [ ] **LAYOUT-09** Lay out for the window, not the screen
- [ ] **LAYOUT-10** One shared compact width
- [ ] **LAYOUT-13** Columns are the split view's

## Controls ([controls.md](../../ui-styleguide/references/controls.md))

- [ ] **CTRL-01** Buttons are kit buttons
- [ ] **CTRL-24** Links go somewhere, buttons do something
- [ ] **CTRL-27** Standard actions use standard glyphs

## Navigation ([navigation.md](../../ui-styleguide/references/navigation.md))

- [ ] **NAV-01** Hierarchy goes sidebar, list, detail
- [ ] **NAV-05** Sidebar rows look like sidebar rows
- [ ] **NAV-09** The back button's text is bound to the previous view's title, not a literal
- [ ] **NAV-16** Segments switch views, not app areas

## Lists ([lists.md](../../ui-styleguide/references/lists.md))

- [ ] **LIST-03** Use the row's properties
- [ ] **LIST-06** An action row is tinted, not chevroned

## Forms ([forms.md](../../ui-styleguide/references/forms.md))

- [ ] **FORM-01** A form is a list of sections, a row per value
- [ ] **FORM-03** The footer is the form's one voice
- [ ] **FORM-14** Paste and drop always work
- [ ] **FORM-17** Validation is client state that mirrors the backend
- [ ] **FORM-19** The sentence goes in the section's footer
- [ ] **FORM-28** Unit-test the validators and the state
- [ ] **FORM-29** Stories and journeys assert what the person sees

## Menus ([menus.md](../../ui-styleguide/references/menus.md))

- [ ] **MENU-01** Menus are the shell's menu

## Feedback, loading, empty, errors ([feedback.md](../../ui-styleguide/references/feedback.md))

- [ ] **FEED-04** Spinners are the kit's

## Help and first run ([help.md](../../ui-styleguide/references/help.md))

- [ ] **HELP-13** No copies of system settings

## Accessibility ([accessibility.md](../../ui-styleguide/references/accessibility.md))

- [ ] **A11Y-11** Focus is always visible

## Pointer, keyboard, windows ([input.md](../../ui-styleguide/references/input.md))

- [ ] **INPUT-04** Shortcuts are declared and shown
- [ ] **INPUT-05** Standard shortcuts keep their meaning
- [ ] **INPUT-06** Few custom shortcuts, made with ⌘
- [ ] **INPUT-17** Scrolling is the system's
- [ ] **INPUT-19** No window chrome of an app's own
