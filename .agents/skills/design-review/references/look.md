# Look: what to check in the pictures

Go through sections 1 to 6 once per story, on its light picture at full size (`__full__light`, or `__ipad__light` for
a component), in the order below: hierarchy, edges and type first, because they are what a person sees first. Then
make the comparisons at the end with the story's other pictures. Section 7 is done once per app, not per story. Each
line names a rule; its full text, with a wrong and a right example, is in the file the heading links to: read those
files before you start, not while you look. A line that does not apply to the story (no form in it) is skipped for
that story only.

For sections 1 and 2, write one line per story even when it passes: the title and its style, the left edges you found
in each column, the rows whose baselines you followed. These are the checks most often waved through.

A picture is drawn at 1×: when an edge or a baseline looks a pixel or two off, confirm it in the source (the padding,
the margin, the `align-items` that places it) rather than guess.

## 1. Hierarchy ([typography.md](../../ui-styleguide/references/typography.md), [layout.md](../../ui-styleguide/references/layout.md))

Rank the texts by how much they stand out (size, then weight, then colour). That order must be the order of importance.

- [ ] **LAYOUT-07** The view's title and its main content are top and leading; inspectors and secondary panels trail or follow.
- [ ] **TYPE-07** At most one Large Title (34, bold) on the screen, and only at a column's root.
- [ ] **NAV-08** A pushed view, a sheet or a detail has the inline title (17 semibold), not a large one.
- [ ] **TYPE-42** Primary text (titles, row titles, the values the person came for, body) is the darkest; supporting text (subtitles, metadata, footers, units) is secondary grey. Never the other way round.
- [ ] **TYPE-16** A date, size, count or author beside a title is smaller or lighter than the title.
- [ ] **TYPE-22** Emphasis inside a block is weight or colour, never a bigger size, and never bold, colour and caps on one phrase.
- [ ] **TYPE-15** What a person reads as the content is Body; nothing they must read is smaller than Footnote.

## 2. Edges and baselines

- [ ] **LAYOUT-06** Follow each column's left edges down the picture. Everything starts on one of two lines: the column's margin (title, search field, an inset group's card) and 16px further in (section headers, each row's first glyph or text, footers). In a plain list the two are one line. Name any third edge.
- [ ] **TYPE-50** Follow each row across: a row's title and value, a bar's title and its text items, a label and its field sit on one baseline; a glyph is centred on its text's line.
- [ ] **TYPE-33** Text is leading-aligned. Centred only in a bar title (centred on the whole bar), an alert, an empty state, a button, a segment, a caption under an image.

## 3. Type for each role ([typography.md](../../ui-styleguide/references/typography.md), its role table)

Name each text's role from the role table; `styles.md` gives its exact numbers.

- [ ] **TYPE-49** The same role is drawn the same way everywhere: every bar title, every bar button, every row title, every section header alike, across stories too.
- [ ] **TYPE-09** A row title is Body regular (Headline only where every row is a name over a preview, as in Mail and Notes).
- [ ] **TYPE-10** A row's second line is Subheadline, secondary.
- [ ] **TYPE-11** A row's trailing value is Body, secondary: never bold, tinted or larger than the title.
- [ ] **TYPE-12** A grouped section header is Footnote caps, secondary.
- [ ] **TYPE-13** A sidebar section header is Headline in the label colour, not caps.
- [ ] **TYPE-14** A section footer is Footnote, secondary, wrapping.
- [ ] **TYPE-18** Only the confirming bar button (Done, Save, Add) is semibold.
- [ ] **TYPE-19** A count badge is Caption 1 semibold, white on red; a count in a sidebar or list is the row's value instead.
- [ ] **TYPE-24** Monospace only for code and machine values (hashes, IDs, ports); never labels, titles or prose.
- [ ] **TYPE-46** Text on a material (a bar, a menu, the Dock) is label black or white and secondary grey, never a fixed grey.
- [ ] **TYPE-45** Text on a photo or wallpaper sits on a capsule, scrim or material, not bare with a shadow.

## 4. Fit

- [ ] **TYPE-32** Messages (footers, alert messages, banners, empty states, errors) wrap; they are never cut.
- [ ] **TYPE-29** A cut name or sentence ends in "…" at its end.
- [ ] **TYPE-30** A path, file name or image tag is cut in the middle, keeping the end that tells items apart.

A title or row cut at full width is a finding by itself: the column is narrower than layout.md's widths (LAYOUT-13).

## 5. Words ([writing.md](../../ui-styleguide/references/writing.md))

Read the strings down the screen in order. For each, name its element type and check its case and ending against
writing.md's table.

- [ ] **WRITE-03** Row titles, field and toggle labels are Title Case (a note's own title is the person's text).
- [ ] **WRITE-06** An alert title that is a fragment is Title Case, no period; a full sentence is sentence case with one.
- [ ] **WRITE-12** A button says what it does, with a verb, in one to three words.
- [ ] **WRITE-07** The fewest plain words; no exclamation marks.
- [ ] **WRITE-09** No "we"; "your" only where the context does not already say whose.
- [ ] **WRITE-20** A name the person gave is in curly quotes in a title or message: `Delete “Shopping”?`
- [ ] **WRITE-24** An error says what failed, in the person's terms, and what to do next.
- [ ] **WRITE-30** An alert message adds a consequence or a reason, or is left out.
- [ ] **WRITE-32** An empty state names what is missing and how to get some.
- [ ] **WRITE-33** A toggle's label names the feature; its footer says what On does.
- [ ] **WRITE-28** No implementation words (RPC, handler, payload) the person did not bring.
- [ ] **WRITE-26** Gender-neutral, no idioms, nothing that assumes ability.
- [ ] **WRITE-34** One term for one thing, and a control named in copy exactly as it is labelled.

## 6. Colour and grouping ([color.md](../../ui-styleguide/references/color.md), [layout.md](../../ui-styleguide/references/layout.md))

- [ ] **COLOR-18** Colour is never the only signal: no dot alone, no red and green that differ only in hue.
- [ ] **LAYOUT-05** Groups are separated by space (the section gap), rows by hairlines; no rules, boxes or shadows between groups.

## 7. Choices (once per app)

Whether a control, a message or a modal is the right one for its job cannot be judged one story at a time. Do this
once per app, over its page stories at full width and in Slide Over, and over the alerts, menus, banners and tooltips
that source.md's first step found in the code (most of them are in no picture). Write one line for each: what it is
and where; the need it serves, in the person's words; the row of the decision table that need leads to; `pass` or the
rule ID. Name the need first and look the row up; don't start from what is on screen.

```
`Delete Folder`, the list's bar | delete this folder | not a main action of the list: More, or the folder's context menu | NAV-13
```

Controls: [controls.md](../../ui-styleguide/references/controls.md)'s "Choosing the control" and "Standard glyphs",
[navigation.md](../../ui-styleguide/references/navigation.md)'s "Where bar items sit". The same table decides CTRL-07,
-11, -12 and -15 (the machine's) and CTRL-13, -17 and -18 (interact.md): a mismatch seen here is a finding even when
the machine passed.
- [ ] **FORM-07** Where the values are known (a protocol, a policy, on or off), a choice, not a text field.
- [ ] **NAV-12** Edit and Add trail the view they change; More is the last item.
- [ ] **NAV-13** A bar holds only the view's main actions, in at most three groups, text items apart; a rare or destructive command is in More or a context menu.
- [ ] **CTRL-05** Buttons are medium; small inside a row or banner; large only for the one main action of an empty state, onboarding or a sheet's bottom.
- [ ] **CTRL-27** A standard action has its standard glyph, and a glyph from the table means nothing else.
- [ ] **CTRL-14** Segment labels are short nouns that fit at the narrowest width.
- [ ] **CTRL-20** A stepper sits right by the value it changes.
- [ ] **CTRL-21** A disclosure row names what it reveals and sits right above it.
- [ ] **CTRL-22** Page dots only for a few peer pages.
- [ ] **CTRL-25** A meter has a name, and its number in text.

Messages and modals: [feedback.md](../../ui-styleguide/references/feedback.md)'s "Where a message goes",
[modals.md](../../ui-styleguide/references/modals.md)'s table.
- [ ] **FEED-01** The volume matches the weight: a routine success is silent, status passive, a failure that blocks work a banner, and only a decision that cannot wait an alert.
- [ ] **FEED-02** An item's status is on its row, not in a panel of its own.
- [ ] **FEED-14** An error shows where it happened: the field's footer, the row, the column's banner.
- [ ] **FEED-15** A dimmed control says why in text nearby, not only in a tooltip.
- [ ] **FEED-16** A failure banner carries its fix (`Retry`).
- [ ] **FEED-18** A toast confirms or offers Undo; it is never an error or a warning.
- [ ] **MODAL-08** An alert only for a problem that needs an answer, or an uncommon destructive act that cannot be undone; a delete that can be recovered (Undo, Recently Deleted) asks nothing.
- [ ] **MODAL-14** Choices about an action the person just started are an action sheet (on iPad, a popover from its button), not an alert.
- [ ] **MODAL-15** In an action sheet the destructive choice is red and first, `Cancel` last (none in the popover form).
- [ ] **MODAL-22** A warning is never a popover.
- [ ] **MODAL-06** A sheet or cover names its task in its title.
- [ ] **MODAL-07** A modal is one view or one straight sequence, with no sidebar or tree of its own.
- [ ] **MODAL-18** A sheet holds one short task, centred at its content's size.
- [ ] **MODAL-23** The whole window is covered only for media or a long, focused edit, with a `Done`.
- [ ] **FEED-22** A notification's title is short, its body sentences, with no app name and no secret.
- [ ] **FEED-23** A red badge counts unread notifications and nothing else.
- [ ] **INPUT-24** An Undo offer names what it undoes (`Undo Delete`).

Lists and forms ([lists.md](../../ui-styleguide/references/lists.md), [forms.md](../../ui-styleguide/references/forms.md))
- [ ] **LIST-02** Inset grouped for settings, forms and an item's attributes; plain for a long list of like items.
- [ ] **LIST-12** A list whose choice shows in the next column keeps the row highlighted; a list of options marks the chosen one with a trailing checkmark and no fill.
- [ ] **LIST-09** A row shows a title and a short preview, never the whole content.
- [ ] **LIST-10** A header is a short noun naming its group, a footer explains it; neither holds a control.
- [ ] **LIST-17** A long list scrolls; no page numbers.
- [ ] **FORM-05** In a form row the label leads and the value trails on the same line; never a label over a field.
- [ ] **FORM-06** Fields are grouped by what they describe, about seven rows to a section at most.
- [ ] **FORM-08** Fields start with sensible defaults.
- [ ] **FORM-04** The commit button comes after the last section (in a sheet, it is the bar's trailing item).
- [ ] **FORM-23** An error says which rule was broken and shows a right value.

Navigation and menus ([navigation.md](../../ui-styleguide/references/navigation.md), [menus.md](../../ui-styleguide/references/menus.md))
- [ ] **NAV-01** Areas in the sidebar, their items in a middle column, the chosen item in the detail; two levels at most in the sidebar.
- [ ] **NAV-05** Sidebar rows are a glyph and a title, the selected one a tint-filled pill with white text; one icon style per sidebar.
- [ ] **NAV-16** Segments switch views of one thing; the app's areas are sidebar rows.
- [ ] **NAV-07** Each view has a bar whose title says where the person is, in under about 15 characters (an open document may have none).
- [ ] **NAV-09** A back button is the chevron and the title of the view it returns to: compare it with that view's picture.
- [ ] **NAV-14** A search field that filters a list sits at the top of its column, under the title; a window-wide one at the bar's trailing end.
- [ ] **MENU-06** In an open menu, the most used items come first, related ones together.
- [ ] **MENU-13** A state in a menu is a checkmark or a label that says it, not both.

Help ([help.md](../../ui-styleguide/references/help.md))
- [ ] **HELP-01** The app opens on its first useful screen: no splash, logo or About card.
- [ ] **HELP-05** Onboarding only for what using the app cannot teach, three short screens at most.
- [ ] **HELP-07** Help explains this app's own tasks, never how standard controls work.
- [ ] **HELP-08** An explanation sits where the question arises: a footer, an empty state's message, a tip by the control.
- [ ] **HELP-11** Nothing a person needs is only in a tooltip (source.md lists them): touch has no hover.
- [ ] **HELP-12** Options about the view (sort, filter, layout) are in the view, not in settings.
- [ ] **HELP-13** No theme, text-size or accent switch of the app's own.
- [ ] **INPUT-19** No title bar or close button of the app's own.

## Comparisons

- **Large text against default:** every text grew (TYPE-02); glyphs that carry meaning grew with it, coloured tiles
  aside; nothing clipped or overlapping (TYPE-26 to -28); the same order of prominence, the primary item still on top.
  - [ ] **TYPE-05** Glyphs that carry meaning grow with the text.
  - [ ] **TYPE-06** The layout changes, not the hierarchy.
- **Light against dark:** nothing vanishes or loses contrast; windows, sheets and popovers sit on the elevated greys,
  not pure black (COLOR-11); grouped rows still stand off their background (COLOR-10).
- **Full against half and Slide Over.** Below 700px the split view is a stack. Look for:
  - one column at a time; each pushed column has a back button labelled with the title of the column it returns to
    (NAV-03, NAV-09);
  - a list keeps its large title under its back button; a detail has an inline title or none (NAV-08);
  - the search field still above the list it filters (NAV-14);
  - the bar on one line: fewer items, the rest moved into More, with the back button, Cancel, the confirming action and
    the trailing items still there (NAV-13); nothing that could be done at full width is gone (LAYOUT-11);
  - no popover: its content is a sheet, and an action sheet rises from the bottom with `Cancel` (MODAL-21, MODAL-15);
  - text 16px or more from the window's edge (LAYOUT-01), nothing cut or crowded.
- **Reduced:** no frosted surface is left (MOTION-03).
  - [ ] **MOTION-12** What motion said (a shake, a bounce) is also said in text or a glyph.
- **Smallest (320 × 240):** nothing overlaps, clips or scrolls sideways (LAYOUT-12).
