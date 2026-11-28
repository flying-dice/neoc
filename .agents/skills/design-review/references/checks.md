# What the machine checks

Every rule of the rulebook whose check is `auto` (`.agents/skills/ui-styleguide/references/auto-checks.md` is the
specification) and what `scripts/capture.ts` with `scripts/audit.ts` does with it. A finding's `check` in
`summary.md` and `findings.json` is the rule ID. Use this table in triage (SKILL.md, step 4) and for the report's Not
checked: an `implemented` rule with no finding passed in every story that shows it; a `partial` rule still needs the
look or the operation in the last column (interact.md's journeys cover them); nothing here replaces looking at the
pictures.

**Status.** `implemented`: the spec's test, with the refinements noted where the literal test would fire on correct
UI. `partial`: the closest sound test; the rest is yours. `none`: not tested (there is none at the moment).

**Severity.** `error`: the rule as specified fails. `warn`: a **review** entry of the spec, or a contrast whose
background cannot be read (an image, a gradient that passes and fails, a material over moving content): confirm it in
the picture.

## Runs

Each run is a fresh load of the story (auto-checks.md, Runs). After a load: two frames, then until no finite
animation runs or 500 ms.

| Run | Setup | Rules |
| --- | --- | --- |
| default | iPad-sized page, light; a story tagged `app` in its AppWindow at full (1180 × 780), Split View half (590 × 780) and Slide Over (375 × 700) | every rule below except those of the other runs |
| dark | the same sizes, `data-theme` dark | the rules whose result depends on the theme: COLOR-02 to -05, -11 to -17, MOTION-03 (b) |
| large-text | `--text-scale: 1.25`, light, first size | TYPE-02 and A11Y-16 (compared with the default run in capture.ts) |
| smallest | a story tagged `app` in a 320 × 240 window (clamped to the app's minimum, as a real window), light | LAYOUT-12 (compared with the full-size default run) |
| reduced | Reduced Motion emulated and `reduce-transparency` on the root, light, first size | A11Y-15, MOTION-03 (c). One load for both settings: neither changes what the other's rule reads |
| operate | after the default run's audit, once per story: Tab walk, hover, toggle clicks, Escape, a collapsed split view, form submit, `contextmenu` events | A11Y-11, A11Y-14, A11Y-18, CTRL-10, FORM-25, INPUT-02, INPUT-09, INPUT-10, MENU-14, MODAL-04, MODAL-05 |

Not applicable to a story, so not built: the iframe per app and the shell's top document (auto-checks.md, Runs). A
story renders an app's page in the Storybook document, inside the AppWindow frame (`[data-testid=app-window]`), whose
content box is the window content root. LAYOUT-08 is partial for that reason, and A11Y-10 reads Storybook's document.

## Scope

- **Views.** Rules about a view in its window (LAYOUT-01, -12, -17, NAV-06, CTRL-03, CTRL-07, FEED-09, -10, -17,
  MODAL-01, MOTION-01, COLOR-10, FORM-02, the window part of CTRL-02 and COLOR-11, and A11Y-01 for kit controls) run
  only in stories tagged `app`. A component showcase is not a view: where a button sits or how big its hit area is
  comes from the view around it.
- **Product copy and colour use.** The Writing rules, COLOR-06 and COLOR-08 run in an app's book (`apps/*`) and in
  stories tagged `app`. In the kit's and the shell's showcases the labels and swatches ("filled", "tinted", a red
  sample) are the story's, not product copy. Built-in copy of a shell component is then `source` (lint-source.ts).
- **App element (A).** Not kit internal and not the story's frame (the AppWindow and its wallpaper, or a showcase's
  top-level wrapper styled inline as a mock screen). In the kit's book only its `ui-*` hosts (the rest is the
  story's scaffolding); the kit's internals are covered by the rules not scoped to A (type, contrast, targets, names).
- **The shell is a design system as the kit is.** In the shell's books (`packages/ui`, `client`, other packages) a
  colour derived from a token (mixed toward black or white, or a token at a lower alpha) passes COLOR-02, COLOR-03
  and MOTION-03 (b), as COLOR-01 lets the kit derive; LAYOUT-02 runs only in an app's book ("the kit's own UIKit
  metrics are the kit's").
- **Names** are Chromium's, read from the accessibility tree over CDP (a kit host's from its inner control). An
  element outside the tree (inert behind an open modal) falls back to a computed name.

## Rules

| ID | Rule | Status | Run | Severity | Notes, and what a reviewer must still do |
| --- | --- | --- | --- | --- | --- |
| TYPE-01 | Every font size is a ramp size | implemented | default | error | A hero number (a calculator display) fails too: the rulebook has no exception. |
| TYPE-02 | Text follows the Text Size setting | implemented | large-text vs default | error | Cross-run in capture.ts: each text of the large-text run against the same element and text at scale 1. |
| TYPE-03 | Line height is the style's leading | implemented | default | error |  |
| TYPE-04 | The smallest text is Caption 2 | implemented | default | error |  |
| TYPE-07 | A large title is Large Title bold | implemented | default | error (weight), warn (more than one) |  |
| TYPE-08 | An inline bar title is Headline | implemented | default | error |  |
| TYPE-09 | A row title is Body | implemented | default | error |  |
| TYPE-16 | Timestamps and metadata are quieter than the title | implemented | default | warn | **review**: confirm the metadata reads quieter. |
| TYPE-18 | The confirming bar button is semibold | implemented | default | error |  |
| TYPE-20 | Only four weights exist | implemented | default | error |  |
| TYPE-21 | Emphasis is one step, by size class | implemented | default | error |  |
| TYPE-23 | Text is in the system font | implemented | default | error |  |
| TYPE-24 | Monospace only for code and data | implemented | default | warn | **review**: is it code or a machine value? |
| TYPE-25 | Letter spacing is the style's tracking | implemented | default | error |  |
| TYPE-26 | Clipped text ends in an ellipsis | implemented | default, large-text, smallest | error | Only when the element's own text runs past its padding box (overflow from a child is not its text). |
| TYPE-27 | Text never spills out of its box | implemented | default, large-text, smallest | error | Clause 2 judges only `overflow: hidden|clip` ancestors (a scroller shows the rest by scrolling), only text partly cut (text clipped away whole is not drawn), and not an ellipsis or line-clamp box (TYPE-26's). Clause 1 needs the element's own text past its box. |
| TYPE-28 | Text never overlaps text or controls | implemented | default, large-text, smallest | error | Compared within one layer (top layer, menu, dialog, fixed, sticky, or absolute with a z-index): an overlay over the page is not an overlap. Rects are cut to what is seen (ellipsis boxes, scrollers). |
| TYPE-32 | Messages wrap, never truncate | implemented | default | error |  |
| TYPE-33 | Text aligns to the leading edge | implemented | default | warn | **review**. Lines are counted from the text's line boxes, not height ÷ line-height. |
| TYPE-34 | Nothing is justified | implemented | default | error |  |
| TYPE-35 | Numbers that line up are tabular | implemented | default | error | One finding per list. |
| TYPE-36 | Body text keeps a readable measure | implemented | default | error |  |
| TYPE-37 | Uppercase only by transform, only for short labels | implemented | default | error |  |
| TYPE-39 | No small caps | implemented | default | error |  |
| TYPE-40 | Italic only in content | implemented | default | error |  |
| TYPE-41 | Underline means a link | implemented | default | error |  |
| TYPE-43 | Tertiary label is never for reading | implemented | default | error |  |
| TYPE-44 | Secondary text is secondary by colour, not opacity | implemented | default | error |  |
| TYPE-47 | Heading levels follow the visual order | implemented | default | error |  |
| WRITE-01 | Titles are title case | implemented | default | error | A sidebar row with custom content: its first text is the title. |
| WRITE-02 | Buttons and menu items are title case | implemented | default | error |  |
| WRITE-03 | Row titles and field labels are title case | implemented | default | warn | **review**: a person's own content may be a row title. |
| WRITE-05 | Messages are sentence case | implemented | default | warn | **review** |
| WRITE-08 | Neutral and calm, never cute | implemented | default | error |  |
| WRITE-09 | No "we", sparing "your" | implemented | default | error | A match next to `- _ . / digit` (an identifier such as `us-east-1`) is not a pronoun. |
| WRITE-10 | Device-neutral verbs | implemented | default | error |  |
| WRITE-11 | Link text says where it goes | implemented | default | error |  |
| WRITE-12 | A button says what it does, with a verb | implemented | default | warn | **review** |
| WRITE-13 | No OK where a verb fits | implemented | default | error | A `ui-alert`'s `actionLabel` is read whether or not it is open (it is what it will say). |
| WRITE-14 | Cancel is always "Cancel" | implemented | default | error | A sheet's leading bar button is judged when it is a dismissal (`Close`, `Dismiss`, `cancel`); a back button (`Back`, `‹`, a `chevron.left`) is MODAL-16's. |
| WRITE-18 | No ending punctuation on labels | implemented | default | error |  |
| WRITE-19 | Typographic punctuation | implemented | default | error |  |
| WRITE-21 | Numbers are numerals, formatted for the locale | implemented | default | error (`(s)`), warn (digits) | **review** for the digits. |
| WRITE-22 | Units are standard and spaced | implemented | default | error |  |
| WRITE-23 | Dates read like a person would say them | implemented | default | error |  |
| WRITE-25 | Errors don't blame | implemented | default | warn | **review** |
| WRITE-26 | Language includes everyone | implemented | default | error |  |
| WRITE-27 | No developer text in the interface | implemented | default | error |  |
| WRITE-29 | An alert title says the situation | implemented | default | error |  |
| WRITE-31 | A placeholder is a hint, not the label | implemented | default | error |  |
| COLOR-02 | Every colour on screen is a token value | implemented | default + dark | error | A slotted text is coloured by the kit element that draws it, not checked as the app's. In the shell's books a colour derived from a token (mixed toward black or white, or at a lower alpha) passes, as COLOR-01 lets the kit derive. |
| COLOR-03 | Text colours come from the label set | implemented | default + dark | error | As COLOR-02 for slotted text and shell derivations. |
| COLOR-04 | Semantic tokens keep their meaning | implemented | default + dark | error | White and black text are not judged as background tokens (white is COLOR-03's); a black terminal or media canvas is not a label-coloured panel. |
| COLOR-05 | Red and blue are the accessible variants | implemented | default + dark | error |  |
| COLOR-06 | Tint means it acts | implemented | default | error | Value equality: `--tint` is also `--ios-blue`, `--link` and `--focus-ring`, so a blue data colour is read as tint; confirm in the picture. |
| COLOR-08 | Red is for destroying and for errors | implemented | default | error |  |
| COLOR-09 | What destroys is red | implemented | default | error |  |
| COLOR-10 | Grouped lists sit on a grouped background | implemented | default (view) | error | Light only: in dark `--bg` and `--bg-grouped` are both black and cannot be told apart. |
| COLOR-11 | Surfaces above the desktop are elevated | implemented | dark | error | A transparent surface is judged by the opaque layer inside it covering 80% of it. |
| COLOR-12 | Lines are separator hairlines | implemented | default + dark | error | A transparent border is no line. |
| COLOR-13 | Fills are for small shapes | implemented | default + dark | error |  |
| COLOR-14 | Text holds 4.5:1 | implemented | default + dark | error, warn (unknown background) | A gradient is judged against every colour stop (all pass, all fail, or review). A backdrop filter (a material over moving content) makes the background unknown, as an image does. |
| COLOR-15 | Large text holds 3:1 | implemented | default + dark | error, warn (unknown background) | As COLOR-14. |
| COLOR-16 | Glyphs and control edges hold 3:1 | implemented | default + dark | error, warn (unknown background) | Inside a disabled control it is exempt (WCAG 1.4.11). |
| COLOR-17 | Placeholders hold 4.5:1 | implemented | default + dark | error, warn (unknown background) |  |
| COLOR-18 | Colour is never the only signal | implemented | default | error | `look` for red and green pairs remains. |
| COLOR-20 | An app never forces a theme on its chrome | implemented | default | error |  |
| LAYOUT-01 | Text keeps off the window's edge | implemented | default (view) | error | Text rects cut to what is seen (an ellipsised title is measured where it ends). |
| LAYOUT-02 | Spacing is on the 4-point grid | implemented | default | error | Computed values from the Typed OM, so an `auto` margin is not a length. Only in an app's book: the kit's and the shell's own UIKit metrics are theirs (the rule says so), and kit hosts are skipped everywhere. |
| LAYOUT-03 | Rows are at least 44 high | implemented | default | error | Explicit roles only (a prose `li` is not a row). |
| LAYOUT-04 | Row content is inset 16 | implemented | default | error |  |
| LAYOUT-06 | Things that belong together align | implemented | default | warn | **review** |
| LAYOUT-08 | Nothing hides under window chrome | partial | default | error | A story has no window chrome: the AppWindow frame draws none, and there is no top document to translate from. It runs on shell stories that draw `-multitasking`/`-resize` elements over their content. Look at a real window's grabber and resize corner over the app's content in the running shell. |
| LAYOUT-12 | The window holds together at its smallest | implemented | smallest vs default | error | The AppWindow clamps 320 × 240 to the app's own minimum size, as a real window does. |
| LAYOUT-14 | Radii come from the tokens | implemented | default | error |  |
| LAYOUT-15 | Nested corners are concentric | implemented | default | error | Judged per corner, where both of that corner's gaps are under R. A capsule or circle has no corner to match and is not judged (the spec's formula would fail any round button near a corner). |
| LAYOUT-16 | Shadows come from the tokens | implemented | default | error |  |
| LAYOUT-17 | No box inside a box | implemented | default (view) | error | The AppWindow frame and its content box count as the window. |
| LAYOUT-18 | Images keep their shape | implemented | default | error |  |
| LAYOUT-19 | Images are sharp | implemented | default | error | Judged for a 2× screen (iPad) whatever the capture's pixel ratio. |
| NAV-02 | The path stays highlighted | implemented | default | error |  |
| NAV-06 | No floating action buttons | implemented | default (view) | error |  |
| NAV-07 | Every view has a bar with a title | implemented | default | warn | **review** |
| NAV-10 | Bar items are borderless | implemented | default | error |  |
| NAV-11 | Cancel leads, the confirming action trails | implemented | default | error |  |
| NAV-12 | Edit and Add sit trailing | implemented | default | error |  |
| NAV-13 | Few items, grouped | implemented | default | warn | **review** |
| CTRL-01 | Buttons are kit buttons | implemented | default | error | Only in an app's book (in the shell's, every button is inside a shell component). |
| CTRL-02 | One prominent button per view | implemented | default | error | The content root in a view; every open modal dialog in any story. One finding per scope. |
| CTRL-03 | Style, not size, ranks a set | implemented | default (view) | error | Compares the size drawn (the label's size, and the height of buttons with a fill): a `small` and a `medium` plain button look the same. |
| CTRL-04 | A destructive button is never the prominent one | implemented | default | error | The focus is read 300 ms after the story rendered; not when its play function has operated the page since (unless the focus is in an open modal). |
| CTRL-07 | A switch lives in a list row | implemented | default (view) | error |  |
| CTRL-08 | A switch has no On/Off text | implemented | default | error |  |
| CTRL-10 | A toggle button changes its fill | partial | operate | error | Judged only where a click really flips `aria-pressed`; a story that does not answer the click leaves both states unknown. Press each toggle button in the app and compare its fill. |
| CTRL-11 | No square checkboxes | implemented | default | error |  |
| CTRL-12 | Two to five segments | implemented | default | error |  |
| CTRL-15 | One of many is a picker | implemented | default | error |  |
| CTRL-16 | A picker always shows a choice | implemented | default | error |  |
| CTRL-23 | Read-only text is a label | implemented | default | error |  |
| CTRL-24 | Links go somewhere, buttons do something | implemented | default | error |  |
| CTRL-25 | A meter has a name and a number | implemented | default | error |  |
| CTRL-26 | Glyphs come from the icon set | implemented | default | error | The shell's `Icon` (`svg.ios-icon`) is the icon set, as `ui-icon` is. A kit control in a row's leading slot is a control, not a glyph. |
| CTRL-28 | Glyph weight matches the text | implemented | default | error |  |
| FORM-02 | On the window's own background, the list is inset | implemented | default (view) | error | Light only, as COLOR-10. |
| FORM-05 | The label leads, the value trails | implemented | default | error |  |
| FORM-09 | The right keyboard and limits | implemented | default | error |  |
| FORM-10 | Identifiers are not capitalised or corrected | implemented | default | error |  |
| FORM-11 | Secrets use secure entry | implemented | default | error |  |
| FORM-12 | Sign-in fields say what they are | implemented | default | error |  |
| FORM-15 | Disabled until there is something to send | implemented | default | error |  |
| FORM-18 | The field shows it is wrong | implemented | default | error |  |
| FORM-19 | The sentence goes in the section's footer | implemented | default | error |  |
| FORM-21 | The field describes its problem to a screen reader | implemented | default | error |  |
| FORM-25 | A banner is for the window, not for input | partial | operate | error | Up to three forms per story: the first required (emptied), `email` or `url` field (given a bad value), Enter in it, 1 s. A form whose commit needs a click, or whose field is invalid another way, is not tried: submit each form invalid in the app and see where the message appears. |
| LIST-01 | Lists are kit lists | implemented | default | error |  |
| LIST-04 | A chevron means it navigates | implemented | default | error |  |
| LIST-06 | An action row is tinted, not chevroned | implemented | default | warn | **review** |
| LIST-07 | One control at a row's end | implemented | default | error |  |
| LIST-11 | The kit draws the separators | implemented | default | error |  |
| LIST-16 | An empty list says so | implemented | default | error |  |
| MODAL-01 | One modal at a time | implemented | default (view) | error |  |
| MODAL-02 | Modals are kit or shell elements | implemented | default | error | `alert`/`confirm`/`prompt` are replaced before the page runs. The overlay part runs in an app's book, and only for an overlay that lies over other content (an app's own root view covers the window too). |
| MODAL-03 | A modal dims what it blocks | implemented | default | error | A shell scrim is `.ios-scrim`. |
| MODAL-04 | Focus stays inside and comes back | partial | operate | error | Only a modal open when the story renders: the focus is inside it 300 ms after, and 20 Tabs keep it inside. Opening a modal from its control and the focus coming back to that control after Escape are not tried: open each modal from the keyboard in the app. |
| MODAL-05 | Escape means Cancel | partial | operate | error | Only what is open when the story renders (the focus put inside it first). A confirm handler is detected for `ui-alert` only. Open each alert, popover and menu in the app and press Escape. |
| MODAL-06 | A modal names its task | implemented | default | error |  |
| MODAL-09 | An alert title fits in two lines | implemented | default | error |  |
| MODAL-10 | Two buttons side by side, three stacked, never more | implemented | default | error |  |
| MODAL-11 | Cancel leads, the likely answer trails | implemented | default | error |  |
| MODAL-12 | A destructive alert has Cancel | implemented | default | error | The focus part as CTRL-04. |
| MODAL-13 | The safe likely answer is the default | implemented | default | error | The focus part as CTRL-04. |
| MODAL-16 | A sheet has Done and Cancel, or Back | implemented | default | error |  |
| MODAL-19 | A popover points at what opened it | implemented | default | error | The opener is known from `aria-controls`, or a single `aria-expanded="true"` element; the last activated control is not tracked. |
| MODAL-21 | Popovers are small and only in regular width | implemented | default | error | A shell `Window` (`.ios-window`, `role=dialog`) is not a popover. |
| MENU-01 | Menus are the shell's menu | implemented | default | error (app), warn (shell) |  |
| MENU-02 | A context menu opens three ways | implemented | default | error | Listeners are recorded by wrapping `addEventListener` before the page runs, plus `oncontextmenu` and Svelte's delegated `__contextmenu` (CDP's listener list misses delegated handlers). A menu's own press-catching layer is skipped. |
| MENU-05 | Short, with few groups | implemented | default | error |  |
| MENU-07 | Destructive items go last | implemented | default | error |  |
| MENU-08 | Item labels are verbs without articles | implemented | default | error |  |
| MENU-09 | Glyphs for all items of a group, or none | implemented | default | error |  |
| MENU-10 | Context menus hide what does not apply | partial | default | error | A context menu is recognised as the shell's `ContextMenu` (a menu in its layer) or a menu whose test id names one; how another menu was opened is not known. Open each context menu and look for disabled items. |
| MENU-11 | No shortcuts in context menus | partial | default | error | As MENU-10. |
| MENU-12 | One level of submenu | implemented | default | error |  |
| MENU-14 | Text keeps its edit menu | implemented | operate | error | Fields, and up to 60 texts selected one by one (at most five findings). |
| FEED-07 | Progress says what it is doing | implemented | default | error |  |
| FEED-09 | Never a blank screen | implemented | default (view) | error | Read 200 ms after the story's render finished. |
| FEED-10 | Content arrives without a jump | partial | default (view) | error | Layout shifts from navigation to the audit (about half a second to a second), not 3 s; not judged when the story's play function operates the page (its shifts follow input). Watch a view load with slow data. |
| FEED-17 | One banner per window | implemented | default (view) | error |  |
| FEED-20 | Feedback is announced | partial | default | error | Static: banner, error-footer and Toast text present must sit in a live region. Text added after load is not observed, and a live region added together with its text is not announced: confirm the region exists before the message arrives (the source, or VoiceOver). |
| HELP-10 | Glyph-only controls name themselves on hover | implemented | default | error | Its name written beside it (a Home Screen label) or in a hover label next to it (the Dock's) is the shell's equivalent of a title. |
| HELP-11 | A tooltip adds, it never carries the essentials | implemented | default | error |  |
| A11Y-01 | Every target is at least 44 × 44 | implemented | default | error | A kit control takes its hit area from `hit` or the row it sits in, so it is judged in views (stories tagged `app`), not in a component showcase. |
| A11Y-02 | Targets do not overlap | implemented | default | error | Within one layer, on the reachable parts of the hit boxes. |
| A11Y-03 | Every control has a name | implemented | default | error | Names from Chromium's accessibility tree (CDP), a kit host's from its inner control. |
| A11Y-04 | A glyph-only control has a real name | implemented | default | error |  |
| A11Y-05 | The name contains the visible label | implemented | default | error | Compared without punctuation; fields (whose label is outside them) are not compared. |
| A11Y-06 | What can be clicked is a control | implemented | default | error | Reported once, on the outermost pointer element (the cursor inherits). |
| A11Y-07 | State is exposed, not only drawn | partial | default | error | (b) is complete. (a) needs `aria-controls` to know which popup is the element's; without it, look at each opener while its menu is open. |
| A11Y-08 | Decorative images are hidden, meaningful ones described | implemented | default | error | `role=presentation|none` hides an svg as `aria-hidden` does. |
| A11Y-09 | Reading order is visual order | implemented | default | error | `order` counts only on a flex or grid item. |
| A11Y-10 | The document says its language | partial | default | error | A story is Storybook's document (`lang="en"`), not the app's: check `lang` in the app's `index.html`. |
| A11Y-11 | Focus is always visible | implemented | operate | error | A Tab walk from the top (at most 300 stops). One whole ring passes: on the element, its kit host, its row's face, a part of it (an app icon's plate), a card around it up to three levels out, its `::before`/`::after` (a window's resize arc), or a slider's thumb (read from the matching style rule). |
| A11Y-12 | No positive tabindex | implemented | default | error |  |
| A11Y-14 | No keyboard traps | partial | operate | error | One Tab walk from the top: a trap is the last 100 stops cycling among 20 or fewer elements that never reach the first. Starting from every control is not tried: Tab out of each widget (editors, terminals, grids). |
| A11Y-15 | Under Reduce Motion nothing moves | implemented | reduced | error |  |
| A11Y-16 | The largest Text Size still fits | implemented | large-text vs default | error | A TYPE-26/27/28 finding of the large-text run that the default run does not have. |
| A11Y-18 | Hover is never required | implemented | operate | error | Hover with the pointer, then focus with `focus()` (the pointer moved away). |
| MOTION-01 | Blur is for what floats | implemented | default (view) | error |  |
| MOTION-02 | Each surface has its material | implemented | default | error | A bar's scroller is scrolled 100 px in the page and back. |
| MOTION-03 | Materials come from the tokens | implemented | default + dark, reduced | error | (a) default, (b) default and dark (a shell colour derived from a token passes, as COLOR-02), (c) reduced: a surface with a translucent background (a clear bar is not a material). |
| MOTION-04 | Bars are clear until content scrolls under them | implemented | default | error |  |
| MOTION-05 | A dim is only a dim | implemented | default | error |  |
| MOTION-06 | Durations are the tokens | implemented | default | error | A loop's period (an infinite animation) is not a transition duration: MOTION-08 judges loops. |
| MOTION-07 | Movement eases like iOS | implemented | default | error | Only for a property with a duration over 0 (every element has `transition-property: all` at 0s). |
| MOTION-08 | Nothing loops but progress | implemented | default | error | A spin by the `rotate` property counts as rotation. |
| INPUT-01 | Controls show the pointer, text shows the I-beam | implemented | default | error | An element interactive only by `tabindex` (a focusable region) is not a control. |
| INPUT-02 | Hover highlights, it doesn't decorate | implemented | operate | error | Up to 40 elements in lists and bars per story. |
| INPUT-03 | Bar targets touch each other | implemented | default | error |  |
| INPUT-07 | One tab stop per group, arrows inside | implemented | default | error | Items are counted once; a control inside an item (a switch at a row's end) is the item's, not the group's. |
| INPUT-09 | Escape closes only the top layer | partial | operate | error | Only when a sheet and a menu or popover over it are open when the story renders. Open a menu in a sheet in the app and press Escape once. |
| INPUT-10 | Focus follows the view | implemented | operate | error | The kit's `ui-split-view[collapsed]`. |
| INPUT-16 | No scroller inside a scroller on the same axis | implemented | default | error |  |
