# Typography

Type is most of what an iPadOS screen is made of, and the first thing that gives away a screen that is not one.
vos has exactly one type system: the Dynamic Type ramp in `packages/ui/src/styles/tokens.css`, set through the
`--type-*` font tokens, the `--tracking-*` tokens, the `.ios-*` classes in `base.css`, the shell's `Text` atom, and the
kit elements, which already apply the right style to each of their parts.

`s` below is the current `--text-scale` (1 by default; Settings > Appearance > Text Size sets 0.85 to 1.25). Every
size and line height in the ramp is multiplied by it.

## The ramp

The values are Apple's "Large (default)" Dynamic Type sizes. A style is a size, a line height (leading), a default
weight, a tracking and a family; it is applied whole, never in pieces.

| Style | Token | Size / leading at `s` = 1 | Default weight | Emphasised weight | Tracking token | Family |
| --- | --- | --- | --- | --- | --- | --- |
| Large Title | `--type-large-title` | 34 / 41 | 400 | 700 | `--tracking-large-title` (0.37px) | display |
| Title 1 | `--type-title1` | 28 / 34 | 400 | 700 | `--tracking-title1` (0.36px) | display |
| Title 2 | `--type-title2` | 22 / 28 | 400 | 700 | `--tracking-title2` (0.35px) | display |
| Title 3 | `--type-title3` | 20 / 25 | 400 | 600 | `--tracking-title3` (0.38px) | display |
| Headline | `--type-headline` | 17 / 22 | 600 | 600 | `--tracking-body` (-0.41px) | text |
| Body | `--type-body` | 17 / 22 | 400 | 600 | `--tracking-body` (-0.41px) | text |
| Callout | `--type-callout` | 16 / 21 | 400 | 600 | `--tracking-callout` (-0.32px) | text |
| Subheadline | `--type-subheadline` | 15 / 20 | 400 | 600 | `--tracking-subheadline` (-0.24px) | text |
| Footnote | `--type-footnote` | 13 / 18 | 400 | 600 | `--tracking-footnote` (-0.08px) | text |
| Caption 1 | `--type-caption1` | 12 / 16 | 400 | 600 | `--tracking-caption` (0px) | text |
| Caption 2 | `--type-caption2` | 11 / 13 | 400 | 600 | 0.07px (`.ios-caption2`) | text |

The tracking values are the repo's tokens. They come from Apple's older split Text/Display table, which matches the
static `SF Pro Text` and `SF Pro Display` the font stack names; the current HIG table, for the variable system font,
differs (notably Title 2 and Title 3, which it tracks negative). The tokens are the design language: a check
compares with `tokens.css`, and if the tokens are brought up to date, the checks follow them.

The `font` shorthand does not set `letter-spacing`. `font: var(--type-…)` alone keeps the tracking it inherits,
usually body's −0.41px, so every style is set as a pair: the font token and its tracking token (or an `.ios-*` class,
or a kit element, that sets both).

Apple's sizes are not one ramp scaled: at its smallest setting Caption 1 and Caption 2 stay at 11 pt. vos multiplies
every size by `s`, so at `s` = 0.85 Caption 2 draws at 9.35px, under the 11 pt minimum (TYPE-04 is checked at `s` = 1):
a gap in the tokens, not something an app works around.

## Which style for which role

| Role | Style, weight, colour | Done by |
| --- | --- | --- |
| Screen title, large | Large Title 700, `--label` | `NavigationBar` large mode, `.ios-large-title` |
| Navigation bar title (inline), sheet title | Headline 600, `--label`, centred | `NavigationBar` inline mode |
| Sidebar section header | Headline 600, `--label`, no transform | `ui-section` in `ui-list variant="sidebar"` |
| Grouped section header | Footnote 400, uppercase, `--label-secondary` | `ui-section header` |
| Section footer | Footnote 400, `--label-secondary` (`--ios-red` for an error) | `ui-section footer` |
| Row title | Body 400, `--label` | `ui-row heading`, `ListRow` |
| Row subtitle | Subheadline 400, `--label-secondary` | `ui-row subheading` |
| Row trailing value | Body 400, `--label-secondary` | `ui-row value` |
| Two numbers at a row's end | Headline + Subheadline secondary, tabular | `ui-value-pair` |
| Running text, a message, a note | Body 400, `--label` | `.ios-body` |
| A paragraph set apart (an explanation above a list) | Callout or Subheadline, `--label-secondary` | `.ios-callout`, `.ios-subheadline` |
| Timestamp, metadata beside a title | Subheadline or Footnote, `--label-secondary`, tabular | `.ios-subheadline` / `.ios-footnote` + `.ios-secondary` |
| Caption, fine print, a chart axis | Caption 1 (Caption 2 at the smallest), `--label-secondary` | `.ios-caption1` |
| Button, medium and small | Subheadline 600 | `ui-button`, `Button` |
| Button, large | Body 600 | `ui-button size="large"` |
| Plain (text) button in content | Subheadline 400 (Body at `size="large"`), `--tint` | `ui-button variant="plain"` |
| Bar button (text), back button | Body 400, `--tint`, as UIKit's bar items | `NavigationBar`; the kit's plain button is Subheadline and has no bar size yet (a kit gap) |
| Confirming bar button (Done, Save) | Body 600, `--tint` | `NavigationBar` trailing item; the kit's plain button has no semibold form yet |
| Alert title / message / buttons | Headline 600 / Footnote 400 / Body 400 (preferred 600), centred | `ui-alert`, `Alert` |
| Menu item | Body 400 | `ContextMenu` |
| Segment label | Footnote 500, selected 600 | `ui-segmented`, `SegmentedControl` |
| Empty state | Title 3 600 + Subheadline `--label-secondary` | `ui-empty-state` |
| Summary tile | Footnote secondary label over a Title 2 tabular value | `ui-tile` |
| Field text / placeholder | Body / Body at 4.5:1 (COLOR-17) | `ui-text-field`, `ui-text-area` |
| Banner / toast | Subheadline (heading 600) | `ui-banner`, `Toast` |
| Count badge on an icon | Caption 1 600, white on `--ios-red`, tabular | (no element yet: add one to the kit first) |

---

## Ramp and Dynamic Type

### TYPE-01 Every font size is a ramp size
- **Rule:** Every text run's computed `font-size` is one of the ramp sizes times `s`: 34, 28, 22, 20, 17, 16, 15, 13,
  12 or 11. Set it with a `--type-*` token, an `.ios-*` class, the `Text` atom or a kit element, never a raw size.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: computed `font-size / s` not within 0.25px of a ramp size.
- **Wrong / Right:** `font-size: 14px;` / `font: var(--type-subheadline); letter-spacing: var(--tracking-subheadline);`

### TYPE-02 Text follows the Text Size setting
- **Rule:** No font size is fixed in `px`, `rem`, `pt` or `vw`: every size scales with `--text-scale`, which only the
  tokens multiply in. An `em` or `%` size counts only if what it is relative to is itself a token size.
- **Why (iPadOS):** Typography; Accessibility.
- **Check:** `auto`: with `--text-scale` set from 1 to 1.25 on the root, every text element's `font-size` grows by a
  factor of 1.25 (within 0.5px).
- **Wrong / Right:** `font: 600 15px/1 var(--font-text);` / `font: var(--type-subheadline); font-weight: 600;`

### TYPE-03 Line height is the style's leading
- **Rule:** The computed `line-height` is the leading paired with the font size in the ramp (41, 34, 28, 25, 22, 21,
  20, 18, 16, 13, times `s`). Never `line-height: 1`, `normal`, or a unitless number of your own.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: `line-height / s` not within 0.5px of the leading for `font-size / s`.
- **Wrong / Right:** `font: var(--type-body); line-height: 1.2;` / `font: var(--type-body);`

### TYPE-04 The smallest text is Caption 2
- **Rule:** Nothing is set smaller than 11px at `s` = 1 (Caption 2), including badges, axis labels, inline code and
  legal text.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: at `s` = 1, any text with computed `font-size` < 11px.
- **Wrong / Right:** `.axis { font-size: 9px }` / `.axis { font: var(--type-caption2); letter-spacing: 0.07px }`

### TYPE-05 Glyphs that carry meaning grow with the text
- **Rule:** An icon that says something next to text (a status glyph, a row's leading symbol that is not a coloured
  tile) is sized relative to the text (`em`, or a size multiplied by `--text-scale`) so it keeps its proportion at
  every Text Size.
- **Why (iPadOS):** Typography; SF Symbols.
- **Check:** `look`: in the large-text pictures (×1.25 is the largest step), a glyph beside text that kept its size;
  `interact`: set Text Size to the largest step and compare glyph and text heights.
- **Wrong / Right:** a 16 px status glyph that stays 16 px while the text beside it grows to 21 px / a glyph sized in
  `em` of that text (the kit's `ui-icon` takes a fixed px `size` today: a gap to close in the kit).

### TYPE-06 At large sizes the layout changes, not the hierarchy
- **Rule:** When the text grows, side-by-side text and values stack (for example `ui-value-pair layout="stacked"`,
  fewer columns) rather than truncating; the primary item stays at the top and keeps its relative prominence.
- **Why (iPadOS):** Typography.
- **Check:** `look`: the large-text pictures against the default ones; `interact`: Text Size at its largest, window at
  compact width.
- **Wrong / Right:** a title cut to "Contai…" beside a fixed 120px value column / the value moves under the title.

## Roles

### TYPE-07 A large title is Large Title bold
- **Rule:** The large title of a top-level screen is Large Title (34/41) at weight 700 in `--label`, from
  `NavigationBar` large mode or `.ios-large-title`; one per screen.
- **Why (iPadOS):** Typography; Toolbars.
- **Check:** `auto`: an element styled as the large title (34×`s` px) has weight 700; `look` for one per screen.
- **Wrong / Right:** `<h1 style="font-size:32px;font-weight:800">Notes</h1>` / `<NavigationBar large title="Notes" />`

### TYPE-08 An inline bar title is Headline
- **Rule:** The title in a navigation bar or a sheet's bar is Headline (17/22, 600), `--label`, centred between the
  bar's leading and trailing items.
- **Why (iPadOS):** Toolbars; Typography.
- **Check:** `auto`: a bar's `h1` whose style is not 17 at weight 600, or, when the bar has items on both sides, whose
  centre is more than 2px from the bar's centre.
- **Wrong / Right:** a 20px bold left-aligned bar title / `NavigationBar` inline title, or an app bar's `h1` in
  `--type-headline` between equal-width sides.

### TYPE-09 A row title is Body
- **Rule:** The primary text of a list row is Body (17/22, 400) in `--label`: `ui-row heading` or `ListRow`. Not
  Headline unless the list's whole point is a name over a preview (Mail, Notes), and then every row does it.
- **Why (iPadOS):** Lists and tables; Typography.
- **Check:** `auto`: the text of a `ui-row`'s `.title` part is 17×`s` px at 400; `look` for hand-made rows.
- **Wrong / Right:** `<div class="row"><b>Wi-Fi</b></div>` / `<ui-row heading="Wi-Fi">`

### TYPE-10 A row subtitle is Subheadline, secondary
- **Rule:** A second line under a row title is Subheadline (15/20, 400) in `--label-secondary`: `ui-row subheading`.
- **Why (iPadOS):** Lists and tables.
- **Check:** `source`, `look`.
- **Wrong / Right:** `<ui-row heading="sshd"><small style="color:gray">PID 702</small></ui-row>` /
  `<ui-row heading="sshd" subheading="PID 702">`

### TYPE-11 A row's trailing value is Body, secondary
- **Rule:** A value at a row's end (`On`, a network name, a count) is Body in `--label-secondary`: `ui-row value`.
  It is never bold, tinted or larger than the title.
- **Why (iPadOS):** Lists and tables.
- **Check:** `source`, `look`.
- **Wrong / Right:** `<span slot="trailing" style="color:var(--tint);font-weight:600">On</span>` /
  `<ui-row heading="Bluetooth" value="On">`

### TYPE-12 A grouped section header is Footnote caps, secondary
- **Rule:** The header over a grouped or inset-grouped section is Footnote, regular, uppercase by `text-transform`,
  in `--label-secondary`: `ui-section header`. Do not restyle it per app.
- **Why (iPadOS):** Lists and tables; Typography.
- **Check:** `source` (no `::part` or wrapper overrides on the header), `look`.
- **Wrong / Right:** `<h3 class="my-header">NETWORK</h3>` above a list / `<ui-section header="Network">`

### TYPE-13 A sidebar section header is Headline
- **Rule:** In a sidebar the section headers are Headline in `--label`, not uppercase: `ui-section` inside
  `ui-list variant="sidebar"` does it.
- **Why (iPadOS):** Sidebars.
- **Check:** `source`, `look`.
- **Wrong / Right:** an uppercase gray footnote header in a sidebar / `<ui-list variant="sidebar"><ui-section header="Library">`

### TYPE-14 A section footer is Footnote, secondary
- **Rule:** A note under a section is Footnote, regular, `--label-secondary`, in sentence case, wrapping to as many
  lines as it needs; `--ios-red` only with `footer-tone="error"`.
- **Why (iPadOS):** Lists and tables.
- **Check:** `source`, `look`.
- **Wrong / Right:** `<p class="hint" style="font-size:12px;color:#999">` under a section / `<ui-section footer="…">`

### TYPE-15 Running text is Body
- **Rule:** Paragraphs, messages, notes and descriptions a person reads as the content are Body in `--label`. A
  supporting paragraph (an intro above a form, help under a control) may be Callout or Subheadline in
  `--label-secondary`; nothing a person must read is smaller than Footnote.
- **Why (iPadOS):** Typography.
- **Check:** `look`.
- **Wrong / Right:** a two-sentence explanation in Caption 2 / the same in Subheadline secondary.

### TYPE-16 Timestamps and metadata are quieter than the title
- **Rule:** A date, size, count or author shown beside a title is Subheadline, Footnote or Caption 1 in
  `--label-secondary`, never larger or heavier than the title it describes.
- **Why (iPadOS):** Typography.
- **Check:** `look`; `auto`: a time-like text (`/\b\d{1,2}:\d{2}\b|yesterday|ago\b/i`) in a row has a smaller
  `font-size` than the row's title.
- **Wrong / Right:** `09:41` in Headline next to a Body title / `09:41` in Subheadline secondary.

### TYPE-17 Buttons take the kit's type
- **Rule:** Button text is what `ui-button` (or the shell `Button`) gives: Subheadline 600 for small and medium, Body
  600 for large, 400 for `variant="plain"`. Never set a button's font in an app.
- **Why (iPadOS):** Buttons.
- **Check:** `source` (no `font` on `ui-button`, its `::part(button)` or a `<button>` styled as one); the computed size is TYPE-01.
- **Wrong / Right:** `ui-button::part(button){font-size:13px}` / `<ui-button size="small">`

### TYPE-18 The confirming bar button is semibold
- **Rule:** In a bar with Cancel and a confirming action (Done, Save, Add), the confirming one is weight 600 and the
  others 400; only one item in a bar is semibold.
- **Why (iPadOS):** Toolbars; Sheets.
- **Check:** `look`; `auto`: more than one text bar item at weight 600 in one bar.
- **Wrong / Right:** both Cancel and Done bold / Cancel 400, Done 600.

### TYPE-19 A count badge is Caption 1 semibold
- **Rule:** A count badge on an icon or tab is Caption 1 at 600, white, tabular, on an `--ios-red` capsule at least
  as wide as it is high. In a sidebar or list, a count is the row's trailing value (TYPE-11), not a badge.
- **Why (iPadOS):** Notifications; Sidebars.
- **Check:** `look`.
- **Wrong / Right:** a blue 10px badge reading "3" in a sidebar row / `<ui-row heading="Inbox" value="3">`

### TYPE-49 One role, one style
- **Rule:** A text takes the style its role has in the role table above, and a role looks the same on every screen of
  the app and in the shell: every bar title one style, every bar button one style, every row title one style. A role
  the table does not name is a gap in the table, not a licence for a style of one app's own.
- **Why (iPadOS):** Typography ("Consider using the built-in text styles", a consistent way to convey hierarchy;
  "Minimize the number of typefaces", or the interface feels internally inconsistent).
- **Check:** `look`: two lines of `styles.md` for one role, or one role drawn two ways in the pictures.
- **Wrong / Right:** `Pin` in Subheadline beside a pop-up in Body in the same bar / every text item of the bar in Body.

## Weight

### TYPE-20 Only four weights exist
- **Rule:** Text weight is 400, 500, 600 or 700. Light weights (100 to 300) and heavy weights (800, 900) are not
  used anywhere.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: computed `font-weight` not in {400, 500, 600, 700}.
- **Wrong / Right:** `font-weight: 300` for a big number / `font-weight: 400`

### TYPE-21 Emphasis is one step, by size class
- **Rule:** Styles from Title 3 down are emphasised with 600 (500 only where the kit uses it, as a segment label);
  never 700. Large Title, Title 1 and Title 2 are emphasised with 700, never 600 or 500.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: `font-size / s` ≤ 20 with weight 700, or ≥ 22 with weight 500 or 600.
- **Wrong / Right:** `h1 { font: var(--type-title3); font-weight: 700 }` / `font-weight: 600`

### TYPE-22 Emphasis is weight or colour, not size
- **Rule:** Within a block of one style, emphasise a word with 600 or with `--label` against `--label-secondary`;
  do not change its size, and do not stack bold, colour and caps on one phrase.
- **Why (iPadOS):** Typography.
- **Check:** `look`.
- **Wrong / Right:** "Your **BACKUP** <big>failed</big>" / "Your backup **failed**."

## Family

### TYPE-23 Text is in the system font
- **Rule:** All interface and content text uses `--font-text` or `--font-display` (the system stack, first family
  `-apple-system`). No web fonts, no other stacks, no `--font-rounded` (it has no use yet).
- **Why (iPadOS):** Typography.
- **Check:** `auto`: the first family of the computed `font-family` is neither `-apple-system` nor `ui-monospace`.
- **Wrong / Right:** `font-family: Inter, sans-serif` / `font: var(--type-body)`

### TYPE-24 Monospace only for code and data
- **Rule:** `--font-mono` is for code, terminal output, logs, and machine values a person compares character by
  character (hashes, IDs, ports in a table). Labels, titles, buttons and prose are never monospace. Monospace text
  still takes a ramp size and leading (TYPE-01, TYPE-03), never `0.88em`.
- **Why (iPadOS):** Typography.
- **Check:** `source`, `look`; `auto` flags for review a mono element that is not `code`, `pre`, `kbd`, `samp`, a
  terminal canvas, or inside one.
- **Wrong / Right:** a monospace section header "ENVIRONMENT" / `<code>sha256:4f2a…</code>` in a row value.

## Tracking

### TYPE-25 Letter spacing is the style's tracking
- **Rule:** Every text run's `letter-spacing` is the tracking token of its style (table above). Set the token with
  the font, or use an `.ios-*` class or kit element that does: the `font` shorthand leaves the inherited tracking in
  place. No tracking of your own, in `em` or for caps.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: computed `letter-spacing` differs by more than 0.02px from the token for the style that
  `font-size / s` maps to (see auto-checks.md for the table).
- **Wrong / Right:** `font: var(--type-title3);` with no tracking (so 0, or inherited -0.41px) /
  `font: var(--type-title3); letter-spacing: var(--tracking-title3);`

## Truncation and wrapping

### TYPE-26 Clipped text ends in an ellipsis
- **Rule:** Text that is kept to one line and can be longer than its box ends in an ellipsis
  (`overflow: hidden; text-overflow: ellipsis; white-space: nowrap`), never cut mid-glyph.
- **Why (iPadOS):** Typography; Labels.
- **Check:** `auto`: an element with text, `overflow` not `visible`, and `scrollWidth > clientWidth + 1`, whose
  `text-overflow` is not `ellipsis`.
- **Wrong / Right:** `.name { overflow: hidden; white-space: nowrap }` / add `text-overflow: ellipsis`.

### TYPE-27 Text never spills out of its box
- **Rule:** No text overflows its container visibly (an unbroken URL, a long word, a translated label): it wraps
  (`overflow-wrap: anywhere` for unbroken strings) or truncates.
- **Why (iPadOS):** Typography; Layout.
- **Check:** `auto`: an element with text and `overflow: visible` whose `scrollWidth > clientWidth + 1`, or a text
  rect that extends past its nearest clipping ancestor.
- **Wrong / Right:** a 64-character token running past the row's edge / `overflow-wrap: anywhere` on its value.

### TYPE-28 Text never overlaps text or controls
- **Rule:** No two text runs, and no text and control, overlap at any window width or Text Size.
- **Why (iPadOS):** Typography; Layout.
- **Check:** `auto`: client rects of text nodes in different elements intersect by more than 1px in both axes, or a
  text rect intersects an interactive element's rect that does not contain it.
- **Wrong / Right:** an absolutely placed badge over a row title / the badge in the row's trailing slot.

### TYPE-29 Tail truncation for names and prose
- **Rule:** Titles, names and sentences truncate at the end ("Quarterly Rep…").
- **Why (iPadOS):** Typography; Lists and tables.
- **Check:** `look`.
- **Wrong / Right:** "…arterly Report" / "Quarterly Rep…"

### TYPE-30 Middle truncation where the end tells items apart
- **Rule:** File names, paths, image tags and other strings whose distinguishing part is the end truncate in the
  middle ("invoice-2026-0…-final.pdf"), done in code because CSS cannot.
- **Why (iPadOS):** Typography; Apple's Files app.
- **Check:** `look`.
- **Wrong / Right:** "registry.example/team/ap…" for ten images that differ only in tag / "registry.exa…/api:1.4.2".

### TYPE-31 What is cut can be read in full
- **Rule:** Truncated text is available whole somewhere: the detail it leads to, a `title` tooltip on pointer hover,
  and its accessible name.
- **Why (iPadOS):** Typography.
- **Check:** `interact`, `source`.
- **Wrong / Right:** a truncated error with nowhere to read the rest / the row opens a detail with the full message.

### TYPE-32 Messages wrap, never truncate
- **Rule:** Section footers, alert messages, banner text, empty-state messages, error text and paragraphs wrap to as
  many lines as they need; only single-line labels and row titles truncate.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: `text-overflow: ellipsis` or `-webkit-line-clamp` on a `ui-section` footer, `ui-alert` message,
  `ui-banner` text, `ui-empty-state` message or `[role=alert]`; `look` elsewhere.
- **Wrong / Right:** a banner cut to "The host refused the con…" / the banner grows to two lines.

## Alignment, numerals, measure, case

### TYPE-33 Text aligns to the leading edge
- **Rule:** Text is start-aligned (`text-align: start`). Centred text is for the elements that centre it on iPadOS:
  a bar title, an alert, an empty state, a button label, a segment label, a short caption under an image.
- **Why (iPadOS):** Layout; Typography.
- **Check:** `look`; `auto`: a centred element of two or more lines outside `ui-alert`, `ui-empty-state`, `Alert`.
- **Wrong / Right:** a centred three-line form explanation / the same, start-aligned.

### TYPE-50 Text side by side shares a baseline
- **Rule:** Texts on one line of a row, a bar or a form (a row's title and its value, a bar's title and its text
  items, a label and the field beside it) sit on one baseline, whatever their sizes; a glyph beside text is centred on
  the text's line, as SF Symbols are.
- **Why (iPadOS):** Layout ("Align elements to make them easier to scan"); SF Symbols (symbols align with text at
  every size).
- **Check:** `look`: follow the baseline across each row; a visible step between neighbours fails. The usual cause is
  `align-items: center` on texts of different sizes or paddings: confirm it in the source.
- **Wrong / Right:** a 15px `Folder` label beside a 17px pop-up, both centred in the bar, their baselines a pixel apart
  / both in one style, or the row aligned with `align-items: baseline`.

### TYPE-34 Nothing is justified
- **Rule:** Never `text-align: justify`.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: computed `text-align` is `justify`.
- **Wrong / Right:** `p { text-align: justify }` / `p { text-align: start }`

### TYPE-35 Numbers that line up are tabular
- **Rule:** Numbers in a column, a table, a counter, a timer or a value that updates in place use
  `font-variant-numeric: tabular-nums` (as `ui-value-pair` and `ui-tile` do), so digits do not jump.
- **Why (iPadOS):** Typography; Apple's Stocks, Clock and Activity Monitor-style tables.
- **Check:** `auto`: in a list with two or more rows whose trailing value is numeric (`/^[\d\s.,:%+\-−]+\s?[A-Za-z%]{0,3}$/`),
  the value's `font-variant-numeric` lacks `tabular-nums`.
- **Wrong / Right:** CPU percentages in proportional digits / `font-variant-numeric: tabular-nums`.

### TYPE-36 Body text keeps a readable measure
- **Rule:** A paragraph of Body or smaller is at most about 80 characters wide: its content box is no wider than
  40em of its own font size (672px at Body, `s` = 1, Apple's readable width).
- **Why (iPadOS):** Layout; Typography.
- **Check:** `auto`: an element whose own text wraps to two or more lines at `font-size` ≤ 20×`s` and whose content
  width exceeds 40 × its `font-size`.
- **Wrong / Right:** an About text across a 1,200px window / `max-inline-size: 40em` on its column.

### TYPE-37 Uppercase only by transform, only for short labels
- **Rule:** `text-transform: uppercase` is for the grouped section header (done by `ui-section`) and short labels
  of a few words at Footnote size or smaller. Never on body copy, sentences, buttons or titles.
- **Why (iPadOS):** Typography; Lists and tables.
- **Check:** `auto`: computed `text-transform: uppercase` on text whose `font-size` > 13×`s`, or longer than 40
  characters, or that wraps to two lines.
- **Wrong / Right:** an uppercase Subheadline warning sentence / sentence case, `--ios-red` if it is an error.

### TYPE-38 Caps are never typed
- **Rule:** Text is written in its natural case in the source; a header that shows in caps gets them from the
  style, so a screen reader and a later restyle read it correctly.
- **Why (iPadOS):** Typography; Accessibility.
- **Check:** `source`: a string literal of two or more words in all caps that are not acronyms.
- **Wrong / Right:** `header="NETWORK SETTINGS"` / `header="Network Settings"`

### TYPE-39 No small caps
- **Rule:** No `font-variant: small-caps` or `font-variant-caps` other than `normal`.
- **Why (iPadOS):** Typography.
- **Check:** `auto`: computed `font-variant-caps` is not `normal`.
- **Wrong / Right:** `font-variant: small-caps` on a label / plain text.

## Emphasis, links and colour

### TYPE-40 Italic only in content
- **Rule:** Interface text (titles, labels, rows, buttons, messages) is never italic. Italic appears only where the
  content itself asks for it: `em`, `i`, `cite` or `blockquote` in text a person or an app wrote.
- **Why (iPadOS):** Typography; Apple's apps.
- **Check:** `auto`: computed `font-style` `italic` or `oblique` on an element that is not, and is not inside,
  `em`, `i`, `cite`, `blockquote` or `[contenteditable]`.
- **Wrong / Right:** an italic "No description" placeholder row / "No Description" in `--label-secondary`.

### TYPE-41 Underline means a link
- **Rule:** Only a link (`a[href]` in running text) may be underlined; headings, buttons, labels and values never.
  Links in running text are `--link`.
- **Why (iPadOS):** Typography; Buttons.
- **Check:** `auto`: computed `text-decoration-line` includes `underline` on an element that is not, and is not
  inside, `a[href]`.
- **Wrong / Right:** `<span style="text-decoration:underline">Required</span>` / `<a href="…">privacy policy</a>`

### TYPE-42 The label level matches the text's importance
- **Rule:** Primary content (titles, row titles, values the person came for, body text) is `--label`; supporting text
  (subtitles, metadata, footers, units) is `--label-secondary`; never the other way round. (Which colours text may
  take at all is COLOR-03.)
- **Why (iPadOS):** Labels; Color.
- **Check:** `look`.
- **Wrong / Right:** a row title in `--label-secondary` under a subtitle in `--label` / title `--label`, subtitle
  `--label-secondary`.

### TYPE-43 Tertiary label is never for reading
- **Rule:** `--label-tertiary` is for disabled text and glyphs such as chevrons only, never for text a person needs
  to read, placeholders included (COLOR-17): it does not reach 4.5:1.
- **Why (iPadOS):** Color; Labels; Accessibility. The repo departs from Apple here: Apple uses tertiary for
  placeholders and unavailable items; vos keeps it off anything readable.
- **Check:** `auto`: a text node in `--label-tertiary` that is not inside a disabled control.
- **Wrong / Right:** a footer note in `--label-tertiary` / `--label-secondary`.

### TYPE-44 Secondary text is secondary by colour, not opacity
- **Rule:** Make text secondary with `--label-secondary`, not with `opacity` on `--label` (opacity also fades
  glyphs, focus rings and children, and changes with the background).
- **Why (iPadOS):** Color.
- **Check:** `source`; `auto`: an element containing text with `opacity` < 1 that is not disabled and not animating.
- **Wrong / Right:** `.meta { opacity: .6 }` / `.meta { color: var(--label-secondary) }`

## Text on images and materials, hierarchy

### TYPE-45 Text on an image sits on something solid
- **Rule:** Text over a photo, wallpaper or video sits on a capsule, a scrim or a material that holds 4.5:1 whatever
  is behind it (as the Home Screen names do); never bare text with a text shadow.
- **Why (iPadOS):** Typography; Images; Accessibility.
- **Check:** `look` (on light and dark wallpaper).
- **Wrong / Right:** white title with `text-shadow` on a photo / white title on `rgb(0 0 0 / .5)` capsule.

### TYPE-46 Text on a material uses label colours
- **Rule:** Text on `--material-*` uses `--label` and `--label-secondary`, which adapt to the theme and to Reduce
  Transparency; never a fixed gray or white.
- **Why (iPadOS):** Materials.
- **Check:** `source`, `look` with Reduce Transparency on and off.
- **Wrong / Right:** `color: rgb(255 255 255 / .7)` on chrome / `color: var(--label-secondary)`

### TYPE-47 Heading levels follow the visual order
- **Rule:** A screen's headings use `h1` to `h3` in the order their sizes suggest, never skipping a level for looks;
  the kit's section headers are already `h3` and alert titles `h2`.
- **Why (iPadOS):** Accessibility; Typography.
- **Check:** `auto`: heading levels in DOM order (crossing shadow roots) skip a level, or a heading is larger than
  an earlier heading of a higher level.
- **Wrong / Right:** `<h4>` styled as a large title / `<h1>` for the screen title.

### TYPE-48 Text people copy is selectable, chrome is not
- **Rule:** Labels and chrome stay unselectable (base.css), but values a person may copy (an IP address, a log line,
  a message, an error detail) are `user-select: text`.
- **Why (iPadOS):** Typography; Apple's apps.
- **Check:** `interact`.
- **Wrong / Right:** a container ID that cannot be copied / its value with `user-select: text`.
