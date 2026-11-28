# Auto checks

The specification for every rule whose check is `auto`: what a script running in the browser tests on computed
styles, geometry and the accessibility tree, crossing open shadow roots. One entry per rule ID, in the order of
`index.md`. A finding names the rule ID, the element (a CSS path through shadow roots), the measured value and the
expected one. Entries marked **review** produce findings a person confirms by looking (the rule's other check); all
others are failures as stated.

## Runs

| Run | Setup | Checks |
| --- | --- | --- |
| Default | `data-theme="light"` on the root, `--text-scale` 1, the window at its default size, motion and transparency not reduced | every entry |
| Dark | `data-theme="dark"` on the root | every entry (colour results differ; geometry repeats) |
| Large text | `--text-scale: 1.25` set on `document.documentElement` | TYPE-02, A11Y-16 |
| Smallest window | the window resized to 320 × 240 px | LAYOUT-12 |
| Reduced motion | `prefers-reduced-motion: reduce` emulated | A11Y-15 |
| Reduced transparency | class `reduce-transparency` on the root | MOTION-03 (c) |

After each setup change, wait two animation frames and until `document.getAnimations()` (and each shadow root's) is
empty or 500 ms have passed.

An app page runs inside its window's iframe (`[data-testid=<app>-frame]`), evaluated in that frame: the frame's
viewport is the window's content area. The shell runs in the top document, where a window's content area is
`[data-testid$="-content"]`. LAYOUT-08 needs rects from both: the runner reads the window's chrome rects in the top
document and translates them into the frame's coordinates.

## Definitions

**s**: `parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--text-scale')) || 1`.

**cs(el)**: `getComputedStyle(el)`; **cs(el, '::placeholder')** for the placeholder.

**walk(root)**: depth-first over the composed tree: every element of `root`, and for each element with an open
`shadowRoot`, every element of that shadow root (slotted children are walked once, as light DOM). Iframes are not
entered; each frame is its own run.

**parent(el)**: `el.assignedSlot ?? el.parentElement ?? (el.getRootNode() instanceof ShadowRoot ? el.getRootNode().host : null)`.
**inside(el, sel)**: `el` or any `parent()` ancestor matches `sel`.

**visible(el)**: `el.getClientRects().length > 0`, `cs(el).visibility === 'visible'`, and no element on the
`parent()` chain (including `el`) has `opacity` 0 or `display: none`.

**Text element (T)**: a visible element with at least one child text node whose `textContent.trim() !== ''`.
**Text rects**: for each such text node, `range.selectNodeContents(node); range.getClientRects()`.
**Lines(el)**: `round(el.getBoundingClientRect().height / parseFloat(cs(el).lineHeight))` for a text element.

**Kit internal**: an element whose root node is a `ShadowRoot` whose host's `localName` starts with `ui-`.
**App element (A)**: in an app frame, any element that is not kit internal (light DOM, kit hosts included). In a
shell run, any element that is not kit internal (the shell's components are the subject).

**Kit hosts (K)**: `ui-button, ui-toggle, ui-check, ui-picker, ui-text-field, ui-text-area, ui-row[tappable]`.

**Interactive (I)**: visible, not disabled (`:disabled`, `[aria-disabled=true]`, inside `[inert]`), and matching
`a[href], button, input:not([type=hidden]), select, textarea, summary, [role=button], [role=link], [role=checkbox],
[role=switch], [role=radio], [role=tab], [role=menuitem], [role=menuitemcheckbox], [role=menuitemradio],
[role=option], [role=slider], [role=spinbutton], [role=combobox], [role=treeitem], [contenteditable=""],
[contenteditable=true], [tabindex]:not([tabindex="-1"])`, or a K host. A kit-internal match whose host is a K host
is replaced by the host (one target per control). Each `[role=radio]` segment of a `ui-segmented` is its own
target.

**Hit box(el)**: a K host's `getBoundingClientRect()`; a `ui-segmented` segment's rect with the top and bottom of its
host; otherwise the element's own `getBoundingClientRect()`.

**Accessible name**: Chromium's computed name from the accessibility tree (CDP
`Accessibility.getPartialAXTree({ backendNodeId })`, `nodes[0].name.value`, trimmed); for a K host, the name of its
inner focus target (the shadow root's `button`, `input`, `textarea`, `select` or `[role=switch]`).

**Visible text(el)**: `el.innerText` (for a K host, the text of its default slot's assigned nodes), trimmed, with runs
of white space collapsed.

**Icon-only**: an element with no visible text whose visible descendants (composed) include a `ui-icon` or `svg`.

**Bar**: an element matching `header, [role=toolbar]`, or the shell's `NavigationBar` row (`.ios-navbar .bar`), that
contains an interactive element or an `h1`/`h2`.
**Alert**: `[role=alertdialog]` that is open (visible). **Modal dialog**: an open `dialog` shown with
`showModal()` (`:modal`), or `[role=dialog][aria-modal=true]`. **Sheet**: a modal dialog that is not an alert.
**Popover**: `:popover-open`, or a visible `[role=dialog]` without `aria-modal` that is `position: absolute|fixed`.
**Menu**: a visible `[role=menu]`; **context menu**: a menu opened by a `contextmenu` event or the Menu key.

**Colour normalisation**: draw the computed colour string on a cleared 1 × 1 canvas (`fillStyle = c; fillRect(0, 0,
1, 1)`) and read `getImageData` → R, G, B (0–255), A (0–1). Two colours are **equal** when |A1 − A2| ≤ 0.02 and every
premultiplied channel (C × A) differs by at most 2.

**Token values**: for the current theme, append a probe `div` to `document.body`; for each token set
`probe.style.color = 'var(--token)'` and normalise `cs(probe).color`; for shadow tokens read `cs(probe).boxShadow` after
`probe.style.boxShadow = 'var(--token)'`; for `--material-blur` read `backdropFilter`. Colour tokens:
`--ios-blue, --ios-green, --ios-indigo, --ios-orange, --ios-pink, --ios-purple, --ios-red, --ios-teal, --ios-yellow,
--ios-mint, --ios-cyan, --ios-brown, --ios-gray, --ios-gray2 … --ios-gray6, --window-grip, --focus-ring, --tint,
--tint-fill, --label, --label-secondary, --label-tertiary, --label-quaternary, --placeholder, --link, --separator,
--separator-opaque, --fill, --fill-secondary, --fill-tertiary, --fill-quaternary, --bg, --bg-secondary,
--bg-tertiary, --bg-grouped, --bg-grouped-secondary, --bg-grouped-tertiary, --bg-elevated, --bg-grouped-elevated,
--bg-grouped-elevated-secondary, --material-ultrathin, --material-thin, --material-regular, --material-thick,
--material-chrome, --dimming, --switch-off, --segment-thumb, --knob`, plus `transparent` and white (`#ffffff`).
**System colours** are the twelve `--ios-*` hues and the six grays.

**Composited background(el)**: walk `parent()` from `el`, collecting each element's normalised `background-color`;
stop at the first with A = 1. If an element on the way has a `background-image` other than `none`, or the walk ends
without an opaque layer, the result is **indeterminate** (report as review, not failure). Composite the collected
layers from the opaque one upwards with source-over. An element's own `opacity` and its ancestors' multiply the text
colour's alpha before compositing it.

**Contrast(fg, bg)**: WCAG 2 relative luminance of each sRGB colour (channel c/255 → c ≤ 0.04045 ? c/12.92 :
((c + 0.055)/1.055)^2.4; L = 0.2126 R + 0.7152 G + 0.0722 B), ratio (L1 + 0.05)/(L2 + 0.05) with L1 the lighter.

**Large text**: `font-size` ≥ 24px, or ≥ 18.66px with `font-weight` ≥ 700.

**Ramp** (size → leading, tracking) at `s` = 1:

| Size | Leading | Tracking | Size | Leading | Tracking |
| --- | --- | --- | --- | --- | --- |
| 34 | 41 | 0.37 | 16 | 21 | −0.32 |
| 28 | 34 | 0.36 | 15 | 20 | −0.24 |
| 22 | 28 | 0.35 | 13 | 18 | −0.08 |
| 20 | 25 | 0.38 | 12 | 16 | 0 |
| 17 | 22 | −0.41 | 11 | 13 | 0.07 |

The tracking column mirrors `--tracking-*` in `tokens.css` (and `.ios-caption2`'s 0.07px); if the tokens change, the
table follows them. **style(el)** is the ramp row whose size is within 0.25px of `parseFloat(cs(el).fontSize) / s`.

**Title-case test**: split the string on white space; strip leading and trailing punctuation (`“”"'’()…:.!?`) from each
word. A word **fails** when it has four or more letters, starts with a lowercase letter, is not in {a, an, the, and,
but, or, nor, for, so, yet, as, at, by, from, in, into, of, off, on, onto, out, over, per, to, up, via, with}, is
not camel case (`/^[a-z]+[A-Z]/`, as `iCloud`), and contains no digit. The first and last word fail on a lowercase
first letter whatever their length (unless camel case).

**Interface text**: text of app elements that are not inside `input, textarea, select, [contenteditable], code, pre,
kbd, samp, [role=log], [role=textbox], canvas` or a terminal (`.xterm`).

---

## Typography

### TYPE-01 Every font size is a ramp size
- Scope: T. Fail: `parseFloat(cs.fontSize) / s` is not within 0.25px of any ramp size.

### TYPE-02 Text follows the Text Size setting
- Scope: T, measured in the default run and the large-text run. Fail:
  `|fs(1.25) − 1.25 × fs(1)| > 0.5px` for the same element.

### TYPE-03 Line height is the style's leading
- Scope: T whose `style()` exists. Fail: `cs.lineHeight` is `normal`, or `|parseFloat(cs.lineHeight) / s − leading| > 0.5`.

### TYPE-04 The smallest text is Caption 2
- Scope: T, default run. Fail: `parseFloat(cs.fontSize) < 10.75`.

### TYPE-07 A large title is Large Title bold
- Scope: T with `style()` = 34. Fail: `cs.fontWeight !== '700'`. Review: more than one such element in a window.

### TYPE-08 An inline bar title is Headline
- Scope: the `h1` of each bar (the inline title `h1` of `NavigationBar`; in large mode its visible inline echo is not
  checked). Fail: `style()` ≠ 17 or weight ≠ 600; or the bar has an interactive item on each side of the `h1` and the
  `h1`'s horizontal centre is more than 2px from the bar's.

### TYPE-09 A row title is Body
- Scope: the `.title` element in each `ui-row`'s shadow root, when present. Fail: `style()` ≠ 17 or weight ≠ 400.
  (Hand-made rows are LIST-01.)

### TYPE-16 Timestamps and metadata are quieter than the title
- Scope: T inside a `ui-row` or `[role=listitem|option|row]` whose visible text matches
  `/\b\d{1,2}:\d{2}\b|\byesterday\b|\bago\b/i`. Fail: its `font-size` ≥ the row title's (`ui-row` `.title`, or the first
  text element of the row) or its weight > the title's. **review**

### TYPE-18 The confirming bar button is semibold
- Scope: each bar. Fail: two or more interactive items with visible text whose computed weight (of the text) is 600.

### TYPE-20 Only four weights exist
- Scope: T. Fail: `cs.fontWeight` not in {400, 500, 600, 700}.

### TYPE-21 Emphasis is one step, by size class
- Scope: T with `style()`. Fail: size ≤ 20 and weight 700; or size ≥ 22 and weight 500 or 600.

### TYPE-23 Text is in the system font
- Scope: T. Fail: the first family of `cs.fontFamily` (split on commas, quotes removed, trimmed) is neither
  `-apple-system` nor `ui-monospace`.

### TYPE-24 Monospace only for code and data
- Scope: T whose first family is `ui-monospace`. Finding when it is not, and is not inside, `code, pre, kbd, samp,
  textarea, [role=log], canvas, .xterm`. **review**

### TYPE-25 Letter spacing is the style's tracking
- Scope: T with `style()`. Fail: `|ls − tracking| > 0.02`, where `ls` is `parseFloat(cs.letterSpacing)` (`normal` = 0).

### TYPE-26 Clipped text ends in an ellipsis
- Scope: T whose `cs.overflowX` is not `visible`. Fail: `el.scrollWidth > el.clientWidth + 1` and `cs.textOverflow !==
  'ellipsis'`.

### TYPE-27 Text never spills out of its box
- Scope: T. Fail: `cs.overflowX === 'visible'` and `el.scrollWidth > el.clientWidth + 1` (for an element whose
  `clientWidth` > 0); or a text rect extends more than 1px past the padding box of its nearest `parent()` ancestor whose
  `overflow` is not `visible`.

### TYPE-28 Text never overlaps text or controls
- Scope: all text rects. Fail: two text rects belonging to different elements, neither inside the other, intersect by
  more than 1px in both axes; or a text rect intersects by more than 1px the hit box of an interactive element that
  does not contain it (the text's own control and its label excepted).

### TYPE-32 Messages wrap, never truncate
- Scope: the `.footer` of each `ui-section`, `#message` of each `ui-alert`, the default slot text of each `ui-banner`,
  the `p` of each `ui-empty-state`, and every `[role=alert]`. Fail: `cs.textOverflow === 'ellipsis'` with
  `white-space: nowrap`, or `-webkit-line-clamp` other than `none`, on it or a descendant text element.

### TYPE-33 Text aligns to the leading edge
- Scope: T with `cs.textAlign === 'center'` and Lines ≥ 2, not inside `[role=alertdialog]`, `ui-empty-state`,
  `ui-button`, `button`. Finding. **review**

### TYPE-34 Nothing is justified
- Scope: T. Fail: `cs.textAlign === 'justify'`.

### TYPE-35 Numbers that line up are tabular
- Scope: each `ui-list` with two or more `ui-row`s whose trailing content (the `value` property, or the visible text of
  the `slot=trailing` element) matches `/^[\d\s.,:%+\-−]+\s?[A-Za-z%]{0,3}$/`. Fail: for any such value element,
  `cs.fontVariantNumeric` does not include `tabular-nums`.

### TYPE-36 Body text keeps a readable measure
- Scope: T with `font-size` ≤ 20 × s and Lines ≥ 2. Fail: content width (`clientWidth − paddingLeft − paddingRight`) >
  40 × `parseFloat(cs.fontSize)`.

### TYPE-37 Uppercase only by transform, only for short labels
- Scope: T with `cs.textTransform === 'uppercase'`. Fail: `font-size` > 13 × s + 0.25, or visible text longer than 40
  characters, or Lines ≥ 2.

### TYPE-39 No small caps
- Scope: T. Fail: `cs.fontVariantCaps !== 'normal'`.

### TYPE-40 Italic only in content
- Scope: T. Fail: `cs.fontStyle` is `italic` or starts with `oblique`, and the element is not, and is not inside,
  `em, i, cite, blockquote, [contenteditable]`.

### TYPE-41 Underline means a link
- Scope: T. Fail: `cs.textDecorationLine` includes `underline` and the element is not, and is not inside, `a[href]`
  (a spelling or grammar marker is not a computed decoration and is not seen).

### TYPE-43 Tertiary label is never for reading
- Scope: T. Fail: normalised `cs.color` equals `--label-tertiary` and the element is not inside a disabled control
  (`:disabled`, `[aria-disabled=true]`, `[disabled]` host).

### TYPE-44 Secondary text is secondary by colour, not opacity
- Scope: T. Fail: the product of `opacity` on the element and its `parent()` chain is < 1, no element on the chain is
  disabled, and `el.getAnimations({ subtree: false })` and the ancestors' are empty.

### TYPE-47 Heading levels follow the visual order
- Scope: visible `h1`–`h6` and `[role=heading][aria-level]` in walk order. Fail: a heading's level is more than one
  greater than the previous heading's; or a heading's `font-size` is larger than that of an earlier heading with a
  smaller level number in the same window.

## Writing

### WRITE-01 Titles are title case
- Scope: the bar's `h1` (and the `NavigationBar` title), the accessible name of each `[role=dialog]`, and the visible
  title of each row in a `ui-list[variant=sidebar]`. Fail: the title-case test fails.

### WRITE-02 Buttons and menu items are title case
- Scope: I with role `button`, `menuitem`, `menuitemcheckbox`, `menuitemradio`, `tab` or a `ui-button` host, that have
  visible text. Fail: the title-case test fails on the visible text.

### WRITE-03 Row titles and field labels are title case
- Scope: each `ui-row` that has a `value`, a `chevron`, or an interactive element in `slot=trailing`. Fail: the
  title-case test fails on its `heading`. **review** (a person's own content can be a row title).

### WRITE-05 Messages are sentence case
- Scope: the message elements listed under TYPE-32. Finding when, ignoring each sentence's first word, three or more
  words start with an uppercase letter and are not all capitals (acronyms). **review**

### WRITE-08 Neutral and calm, never cute
- Scope: interface text. Fail: matches `/\b(oops|uh-?oh|whoops|yikes|yay|woohoo)\b/i` or contains a code point with
  the Unicode property `Extended_Pictographic`.

### WRITE-09 No "we", sparing "your"
- Scope: interface text. Fail: matches `/\b(we|we're|we’re|we've|we’ve|our|us)\b/i`.

### WRITE-10 Device-neutral verbs
- Scope: interface text. Fail: matches `/\b(click|tap|right-click|double-click)(s|ed|ing)?\b/i`.

### WRITE-11 Link text says where it goes
- Scope: `a[href]`. Fail: accessible name matches `/^(click here|here|this|link|more)$/i`.

### WRITE-12 A button says what it does, with a verb
- Scope: I with role `button` or a `ui-button` host, with visible text. Finding when the text has more than three
  words. **review**

### WRITE-13 No OK where a verb fits
- Scope: alerts and buttons. Fail: a `ui-alert` whose `actionLabel` is `OK` (its default included); an alert with two
  or more buttons one of which is named `OK`; any button named exactly `Yes` or `No`.

### WRITE-14 Cancel is always "Cancel"
- Scope: the `cancel` part of each `ui-alert`, a shell `Alert` action of style `cancel` (the button bolded as cancel,
  or its `data-testid` ending `-action-cancel`), and the leading bar button of each sheet. Fail: its name is not
  exactly `Cancel`.

### WRITE-18 No ending punctuation on labels
- Scope: accessible names of I with role `button`, `menuitem*`, `tab`, `radio` (segments), headings, and `ui-row`
  `heading` values. Fail: ends in `.`, `:` or `!` (not `…`).

### WRITE-19 Typographic punctuation
- Scope: interface text nodes. Fail: contains `...`, `"` or `'`.

### WRITE-21 Numbers are numerals, formatted for the locale
- Scope: interface text. Fail: matches `/\(s\)/`, or contains `/(?<![\d.,])\d{5,}(?![\d.,])/` (an ungrouped run of five
  or more digits) not inside an identifier-looking token (`/[A-Za-z_-]\d|\d[A-Za-z_-]/`). **review** for the digits.

### WRITE-22 Units are standard and spaced
- Scope: interface text. Fail: matches `/\d(KB|MB|GB|TB|ms)\b/` or `/\d\s%/`.

### WRITE-23 Dates read like a person would say them
- Scope: interface text. Fail: matches `/\d{4}-\d{2}-\d{2}T\d{2}:/` or `/\b\d{10}(\d{3})?\b/`.

### WRITE-25 Errors don't blame
- Scope: text of `ui-section[footer-tone=error]` footers, `[role=alert]`, `ui-banner[tone=error]`. Finding when it
  matches `/\b(invalid|illegal|wrong|bad|you failed|not allowed)\b/i`. **review**

### WRITE-26 Language includes everyone
- Scope: interface text. Fail: matches `/\b(he or she|his or her|guys|whitelist|blacklist|master|slave)\b/i`.

### WRITE-27 No developer text in the interface
- Scope: interface text. Fail: matches
  `/\b(undefined|null|NaN|Exception|Traceback)\b|\[object Object\]|\bE[A-Z]{4,}\b|-32\d{3}\b|^Error:/m`.

### WRITE-29 An alert title says the situation
- Scope: the accessible name of each alert. Fail: matches `/^(error|warning|alert|confirm|are you sure\??|attention)$/i`.

### WRITE-31 A placeholder is a hint, not the label
- Scope: `input`, `textarea` (inner targets of `ui-text-field`, `ui-text-area` included) with a non-empty
  `placeholder`. Fail: the accessible name is empty or equals the placeholder.

## Color

### COLOR-02 Every colour on screen is a token value
- Scope: visible A. Properties: `color` (T only), `background-color`, `border-*-color` (sides with width > 0 and style
  not `none`), `outline-color` (when `outline-style` is not `none`), `fill` and `stroke` (SVG elements, not `none`).
  Fail: a non-transparent value not equal to any token value.

### COLOR-03 Text colours come from the label set
- Scope: T in A. Fail: `cs.color` equals none of `--label`, `--label-secondary`, `--tint`, `--link`, `--ios-red`,
  `--label-tertiary` (TYPE-43 owns it), and is not white over a composited background equal to `--tint`, `--tint-fill`
  or a system colour.

### COLOR-04 Semantic tokens keep their meaning
- Scope: A. Fail: T whose `color` equals a `--bg*`, `--fill*` or `--separator*` value (and not also a label value);
  an element of at least 24 × 24px whose `background-color` equals a `--label*` value.

### COLOR-05 Red and blue are the accessible variants
- Scope: all visible elements, every colour property listed in COLOR-02. Fail: equals `#ff3b30`, `#ff453a`, `#007aff`
  or `#0a84ff`.

### COLOR-06 Tint means it acts
- Scope: A whose `color` (T) or `background-color` equals `--tint` or `--tint-fill`. Fail: not inside I, `ui-segmented`,
  `ui-toggle`, `ui-check`, `ui-row[action]`, `ui-row[selected]`, `[aria-selected=true]`, or a `ui-meter`.

### COLOR-08 Red is for destroying and for errors
- Scope: A whose `color` or `background-color` equals `--ios-red`. Fail: not inside `[destructive]`, `.destructive`,
  `[footer-tone=error]`, `[invalid]`, `[aria-invalid=true]`, `[role=alert]`, `ui-banner[tone=error]`, a `ui-row` icon
  tile or a chart (`svg` with `role=img`).

### COLOR-09 What destroys is red
- Scope: menu items, alert buttons, and `ui-row[tappable]` whose accessible name matches
  `/^(Delete|Erase|Remove|Discard|Empty Trash|Reset)\b/`. Fail: the text colour is not `--ios-red`.

### COLOR-10 Grouped lists sit on a grouped background
- Scope: `ui-list[variant=insetGrouped]`, `ui-list:not([variant])`, `ui-list[variant=grouped]`. Fail: the first
  `parent()` ancestor with an opaque background is not `--bg-grouped` or `--bg-grouped-elevated`.

### COLOR-11 Surfaces above the desktop are elevated
- Scope: dark run; modal dialogs, popovers, and the window content root (the frame's `body`, or the shell window's
  content). Fail: background (or, if transparent, its first opaque ancestor inside the surface) equals `#000000`,
  unless the surface's visible area is at least 80% covered by a `canvas`, `video` or `.xterm`.

### COLOR-12 Lines are separator hairlines
- Scope: A. Fail: a border side with width ≤ 2px and style not `none` whose colour is not `--separator`,
  `--separator-opaque`, `--ios-red` or `--focus-ring`; or an element ≤ 2px high (or wide) and ≥ 24px long whose
  background is not one of those.

### COLOR-13 Fills are for small shapes
- Scope: A. Fail: `width × height > 40000` and `background-color` equals a `--fill*` value.

### COLOR-14 Text holds 4.5:1
- Scope: T, not large text, not inside a disabled control. Fail: Contrast(composited text colour, composited
  background) < 4.5. Indeterminate backgrounds are review.

### COLOR-15 Large text holds 3:1
- Scope: T that is large text, not disabled. Fail: Contrast < 3.

### COLOR-16 Glyphs and control edges hold 3:1
- Scope: visible `svg` (including in `ui-icon`) that has an accessible name or is the only visible content of an I.
  Colour: its `stroke` if not `none`, else its `fill`, resolving `currentColor` to `cs.color`. Fail: Contrast against the
  composited background < 3.

### COLOR-17 Placeholders hold 4.5:1
- Scope: `input`, `textarea` with a non-empty placeholder and an empty value (inner targets of kit fields included).
  Fail: Contrast(`cs(el, '::placeholder').color` composited, composited background of the field) < 4.5.

### COLOR-18 Colour is never the only signal
- Scope: visible elements of at most 16 × 16px whose `background-color` is a system colour. Fail: no visible text, no
  accessible name (`aria-label`, `title`, role `img` with a name), and no visible text in the parent's other children.

### COLOR-20 An app never forces a theme on its chrome
- Scope: A with a `data-theme` attribute, or with `cs.colorScheme` equal to `light` or `dark` (not `light dark`) that
  differs from the root's, excluding the root. Fail: it contains a bar, a `ui-list` or a `ui-button`.

## Layout

### LAYOUT-01 Text keeps off the window's edge
- Scope: text rects. Fail: `rect.left < 16` or `rect.right > innerWidth − 16` (frame coordinates; in the shell, the
  window content rect's edges ± 16), for text not inside a `ui-row` (its insets are the kit's).

### LAYOUT-02 Spacing is on the 4-point grid
- Scope: visible A that are not K hosts. Properties: `margin-*`, `padding-*`, `gap`, `row-gap`, `column-gap` in px
  (`normal` = 0). Fail: `v = |value|` is not within 0.5 of a multiple of 4, and `v` not in {0, 0.5, 1, 2}.

### LAYOUT-03 Rows are at least 44 high
- Scope: visible `ui-row` hosts, and elements with role `listitem`, `option`, `row`, `menuitem`, `treeitem` (kit
  internal excluded). Fail: `getBoundingClientRect().height < 43.5`.

### LAYOUT-04 Row content is inset 16
- Scope: A with role `listitem`, `option` or `row` that are not `ui-row` hosts and not inside a `ui-row`. Fail: the
  leftmost child content rect (text rect or element rect) starts less than 15.5 or more than 16.5px from the row's
  left edge.

### LAYOUT-06 Things that belong together align
- Scope: an A `h2`–`h4` immediately followed by a sibling list (`[role=list]`, `ul`, `ol`) with item text. Finding when
  the heading's first text rect left differs from the first item's first text rect left by more than 1px. **review**

### LAYOUT-08 Nothing hides under window chrome
- Scope: I and text rects of the page; the window's `[data-testid$="-multitasking"]` and `[data-testid$="-resize"]`
  rects from the top document, translated. Fail: any intersection greater than 1px in both axes.

### LAYOUT-12 The window holds together at its smallest
- Scope: smallest-window run. Fail: the content root's `scrollWidth > clientWidth + 1`; or a TYPE-27 or TYPE-28 failure
  that does not occur in the default run.

### LAYOUT-14 Radii come from the tokens
- Scope: visible A with a non-zero `border-*-radius`. For each corner value in px: fail unless it is within 0.5 of
  {6, 7, 8, 10, 12, 14, 18, 22, 30} or ≥ half the element's shorter side − 0.5; a `%` value fails unless 50% or
  22.37%.

### LAYOUT-15 Nested corners are concentric
- Scope: pairs of an A element E with radius r > 0 and its nearest `parent()` ancestor P with radius R > 0, where E's
  rect is inside P's and g = the smallest of E's four distances to P's edges, g < R. Fail: r > R − g + 1.

### LAYOUT-16 Shadows come from the tokens
- Scope: visible A with `cs.boxShadow !== 'none'`. Fail: the value equals none of the six shadow token values, is not
  an `inset` shadow, and is not a single shadow with zero blur and spread whose colour is a separator token (a hairline).

### LAYOUT-17 No box inside a box
- Scope: A "boxes": elements with a visible border or a background different from their parent's composited
  background, a non-zero radius, and at least 64 × 32px. Fail: a box whose nearest box ancestor is a box too, when that
  ancestor is not a modal dialog, popover, menu, the window content root, or a `ui-section`.

### LAYOUT-18 Images keep their shape
- Scope: visible `img` (complete, `naturalWidth` > 0) and `video` (`videoWidth` > 0). Fail: `cs.objectFit === 'fill'`
  and `|(w/h) / (natW/natH) − 1| > 0.01`.

### LAYOUT-19 Images are sharp
- Scope: visible `img` whose `currentSrc` does not end in `.svg` and is not `data:image/svg+xml`. Fail: `naturalWidth <
  0.9 × renderedWidth × devicePixelRatio`.

## Navigation

### NAV-02 The path stays highlighted
- Scope: each `ui-split-view` without `collapsed` whose default slot (the detail) holds visible content other than a
  `ui-empty-state`. Fail: the `ui-list` in its `sidebar` slot (and in its `content` slot, if any) has no row with
  `selected`.

### NAV-06 No floating action buttons
- Scope: I with role `button` or `ui-button` hosts, whose own `position` or an ancestor's (up to the content root) is
  `fixed`, `absolute` or `sticky`, not inside a bar, popover, menu or dialog. Fail: its hit box is within 48px of the
  content root's bottom or right edge and its radius is ≥ half its shorter side.

### NAV-07 Every view has a bar with a title
- Scope: the `h1` of each bar. Finding when its visible text is longer than 24 characters. **review**

### NAV-10 Bar items are borderless
- Scope: I inside bars. Fail: a `ui-button` whose `variant` is not `plain`; or a non-kit control whose resting
  `background-color` is not transparent or whose border is visible.

### NAV-11 Cancel leads, the confirming action trails
- Scope: each bar. Fail: an item named `Cancel` whose hit box `left` is greater than that of an item named `Done`,
  `Save`, `Add` or `Send`.

### NAV-12 Edit and Add sit trailing
- Scope: each bar. Fail: an item named `Edit` or `Add` (or icon-only with a `ui-icon[name=plus]`) whose centre is left of
  the bar's centre.

### NAV-13 Few items, grouped
- Scope: each bar. Finding when it holds more than five I. **review**

## Controls

### CTRL-01 Buttons are kit buttons
- Scope: A matching `button, [role=button], a[href]` that are not kit internal and not inside a shell component (app
  runs: every light-DOM match). Fail: a resting `background-color` that is not transparent, a visible border, or a
  non-zero radius.

### CTRL-02 One prominent button per view
- Scope: each window content root and each modal dialog. Fail: more than one visible `ui-button` whose `variant`
  attribute is absent or not one of `tinted`, `gray`, `plain`.

### CTRL-03 Style, not size, ranks a set
- Scope: groups of two or more visible `ui-button`s sharing a parent whose vertical centres are within 4px of each
  other. Fail: their `size` attributes (absent = `medium`) differ.

### CTRL-04 A destructive button is never the prominent one
- Scope: `ui-button[destructive]`. Fail: `variant` absent or `filled`; or it is the focused element (through shadow
  roots) within 300 ms of its window, sheet or alert opening.

### CTRL-07 A switch lives in a list row
- Scope: visible `ui-toggle` hosts and `[role=switch]` not kit internal. Fail: no `ui-row` and no `[role=listitem]`
  `parent()` ancestor.

### CTRL-08 A switch has no On/Off text
- Scope: `ui-row`s containing a `ui-toggle`. Fail: the row's `value`, or visible text in `slot=trailing` other than the
  toggle, matches `/^(On|Off|Enabled|Disabled)$/i`.

### CTRL-10 A toggle button changes its fill
- Scope: elements with `aria-pressed` (kit internal included). Method: record the normalised `background-color` with
  `aria-pressed` `false` and `true` (toggle by clicking, restore after). Fail: equal.

### CTRL-11 No square checkboxes
- Scope: visible `input[type=checkbox]`, `input[type=radio]` that are not kit internal. Fail: any.

### CTRL-12 Two to five segments
- Scope: `ui-segmented` hosts; `[role=radiogroup]` not kit internal. Fail: fewer than 2 or more than 5 radios.

### CTRL-15 One of many is a picker
- Scope: visible `select` that is not kit internal. Fail: any.

### CTRL-16 A picker always shows a choice
- Scope: `ui-picker` without `required`. Fail: its `value` equals no option's `value` (the inner `select` has
  `selectedIndex` −1).

### CTRL-23 Read-only text is a label
- Scope: visible `ui-text-field[readonly]`, `ui-text-area[readonly]`, and `input[readonly]`, `textarea[readonly]`
  that are not kit internal. Fail: any.

### CTRL-24 Links go somewhere, buttons do something
- Scope: A. Fail: an `a` without `href`; an element with `role=link` that is not an `a[href]`.

### CTRL-25 A meter has a name and a number
- Scope: `ui-meter` with a numeric `value`. Fail: empty `label`.

### CTRL-26 Glyphs come from the icon set
- Scope: I and the leading slot of each `ui-row`. Fail: the only visible content is an `img`, an `svg` that is not in a
  `ui-icon`, or text consisting only of `Extended_Pictographic` characters.

### CTRL-28 Glyph weight matches the text
- Scope: `ui-icon` hosts that are direct content of items of one bar, or of the rows of one `ui-list` (same slot).
  Fail: their `size` or `weight` properties differ within the group.

## Forms

### FORM-02 On the window's own background, the list is inset
- Scope: `ui-list[variant=inset]`. Fail: its first opaque ancestor background is `--bg-grouped` or
  `--bg-grouped-elevated`.

### FORM-05 The label leads, the value trails
- Scope: `ui-text-field` inside a `ui-row` with a `heading`. Fail: the field's top is below the bottom of the row's
  `.title` rect.

### FORM-09 The right keyboard and limits
- Scope: `ui-text-field` hosts (and plain `input`s) with an accessible name. Fail: name matches `/e-?mail/i` and `type`
  ≠ `email`; or `/\b(url|website|link)\b/i` and `type` ≠ `url`; or
  `/\b(port|count|number|pid|ttl|size|replicas|priority)\b/i` and `type` ≠ `number` and `inputmode` not `numeric` or
  `decimal`.

### FORM-10 Identifiers are not capitalised or corrected
- Scope: as FORM-09. Fail: (`type` is `email` or `url`, or the name matches
  `/\b(host|user ?name|path|domain|token|key|id|image|tag)\b/i`) and `autocapitalize` is not `off` or `none`.

### FORM-11 Secrets use secure entry
- Scope: as FORM-09. Fail: name matches `/\b(password|passphrase|secret|token|api key|private key)\b/i` and `type` ≠
  `password`; or `type` = `password` and the value is non-empty on the first check after its view opens.

### FORM-12 Sign-in fields say what they are
- Scope: `type=password` fields. Fail: empty `autocomplete`.

### FORM-15 Disabled until there is something to send
- Scope: forms (`form`, or a `ui-list` followed by a submit `ui-button`) with a `required` field whose value is empty.
  Fail: the form's submit button (`type=submit`, or the last `ui-button` after the list) is not `disabled`.

### FORM-18 The field shows it is wrong
- Scope: `ui-section[footer-tone=error]` with a visible footer. Fail: no control in it has `invalid` or
  `aria-invalid="true"`.

### FORM-19 The sentence goes in the section's footer
- Scope: controls with `invalid` (or inner `aria-invalid=true`) inside a `ui-section`. Fail: the section's
  `footerTone` is not `error` or its footer is empty.

### FORM-21 The field describes its problem to a screen reader
- Scope: as FORM-19. Fail: the inner control's `aria-description` is empty or differs (trimmed) from the section's
  `footer`.

### FORM-25 A banner is for the window, not for input
- Method: for each form, make one field invalid, submit (Enter in the field, or click the commit button with
  `disabled` forced off), wait 1 s. Fail: a `ui-banner` that was not visible before is visible.

## Lists

### LIST-01 Lists are kit lists
- Scope: A that are not kit internal and not inside a `ui-row` or shell `ListRow`. Fail: role `listitem`, `option` or
  `row`; or an `li` with a non-transparent background or a visible border.

### LIST-04 A chevron means it navigates
- Scope: `ui-row[chevron]`. Fail: not `tappable`; or `action` or `destructive`; or its `slot=trailing` holds an I.

### LIST-06 An action row is tinted, not chevroned
- Scope: `ui-row[tappable]` whose `heading` matches
  `/^(Add|New|Create|Restart|Start|Stop|Sign|Log|Connect|Disconnect|Reset|Export|Import|Run|Check|Install)\b/`. Finding
  when it has none of `action`, `destructive`, `chevron`. **review**

### LIST-07 One control at a row's end
- Scope: `ui-row`. Fail: the elements in `slot=trailing` contain more than one I.

### LIST-11 The kit draws the separators
- Scope: `ui-row` hosts and their light-DOM parent when that parent is not `ui-section`/`ui-list`. Fail: a visible
  border, a `box-shadow` other than `none`, or a vertical margin other than 0 on a `ui-row`.

### LIST-16 An empty list says so
- Scope: visible `ui-list`. Fail: no visible `ui-row` descendant and no visible `ui-empty-state` in the same
  `ui-split-view` column (slot) or, outside a split view, in the same parent.

## Modals

### MODAL-01 One modal at a time
- Scope: each window. Fail: the number of open modal dialogs, popovers and menus is more than one, unless it is
  exactly one alert above exactly one sheet.

### MODAL-02 Modals are kit or shell elements
- Method: before the page runs, replace `window.alert`, `window.confirm`, `window.prompt` with recorders. Fail: any
  call; or an A with `position` `fixed` or `absolute` whose rect covers ≥ 80% of the viewport and whose background
  alpha is ≥ 0.2, that is not a kit or shell component.

### MODAL-03 A modal dims what it blocks
- Scope: open modal dialogs; popovers; menus. Fail: a modal `dialog`'s `::backdrop` background (`cs(dialog,
  '::backdrop')`), or the scrim directly behind a shell modal, is not `--dimming`; a popover or menu with a visible
  full-viewport scrim (an element covering ≥ 80% with background alpha > 0) beneath it.

### MODAL-04 Focus stays inside and comes back
- Method: focus the control that opens a modal, activate it with Enter. Fail: after 300 ms the deepest focused element
  (through shadow roots) is not inside the dialog; any of 20 Tab presses moves it outside; after Escape, the focused
  element is not the opener.

### MODAL-05 Escape means Cancel
- Method: open each alert, popover and menu; press Escape. Fail: it is still open after 300 ms, or its confirm handler
  ran (`confirm` event on `ui-alert`, or the action's handler for a shell `Alert`).

### MODAL-06 A modal names its task
- Scope: open `[role=dialog]`. Fail: empty accessible name.

### MODAL-09 An alert title fits in two lines
- Scope: alerts, default run. Fail: the title's height > 2 × its line height + 1; or the alert's text container has
  `scrollHeight > clientHeight + 1`.

### MODAL-10 Two buttons side by side, three stacked, never more
- Scope: alerts. Fail: more than three I; or three I whose vertical centres are within 4px of each other.

### MODAL-11 Cancel leads, the likely answer trails
- Scope: alerts with a button named `Cancel`. Fail: with two buttons in a row, `Cancel` is not the leftmost; with
  buttons stacked, `Cancel` is not the lowest.

### MODAL-12 A destructive alert has Cancel
- Scope: alerts with a button whose text colour is `--ios-red`. Fail: no button named `Cancel`; or the red button is
  focused 300 ms after opening.

### MODAL-13 The safe likely answer is the default
- Scope: two-button alerts with no red button. Fail: the focused button 300 ms after opening is `Cancel`, or the button
  with weight 600 is `Cancel`.

### MODAL-16 A sheet has Done and Cancel, or Back
- Scope: the bar of each sheet. Fail: an item named `Done`, `Save` or `Add` and none named `Cancel` or starting `‹`/named
  `Back`; or items named `Cancel`, `Back` and `Done` all present.

### MODAL-19 A popover points at what opened it
- Scope: popovers whose opener is known (`aria-controls`/`aria-expanded` owner, or the last activated control). Fail:
  the popover's rect intersects the opener's hit box by more than 1px.

### MODAL-21 Popovers are small and only in regular width
- Scope: popovers. Fail: width > 400px or > 50% of the viewport width; or the viewport is narrower than 700px.

## Menus

### MENU-01 Menus are the shell's menu
- Scope: `[role=menu]` in an app frame (not kit internal). Fail: any; and in the shell, a `[role=menu]` that is not
  the `ContextMenu` or `MultitaskingMenu` (their `data-testid`s). **review** in the shell.

### MENU-02 A context menu opens three ways
- Method: list elements with a `contextmenu` listener (CDP `DOMDebugger.getEventListeners`). Fail: no `aria-haspopup`
  on the element or its interactive descendant.

### MENU-05 Short, with few groups
- Scope: menus. Fail: more than 12 `menuitem*` or more than 3 groups (groups are split by `[role=separator]` or by a
  visible gap of 6px or more between items).

### MENU-07 Destructive items go last
- Scope: menus. Fail: an item whose name matches `/^(Delete|Remove|Erase|Discard|Move to Trash)\b/` is followed (in DOM
  order) by an item whose text is not `--ios-red`.

### MENU-08 Item labels are verbs without articles
- Scope: `menuitem*`. Fail: name matches `/\s(the|a|an)\s/i`.

### MENU-09 Glyphs for all items of a group, or none
- Scope: each menu group (MENU-05). Fail: some items contain a visible `svg`/`ui-icon` and others do not.

### MENU-10 Context menus hide what does not apply
- Scope: context menus. Fail: an item with `disabled` or `aria-disabled="true"`.

### MENU-11 No shortcuts in context menus
- Scope: context menus. Fail: an item's visible text contains `⌘`, `⌥`, `⌃` or `⇧`.

### MENU-12 One level of submenu
- Scope: menus. Fail: an item with `aria-haspopup` inside a menu that is itself a submenu (opened from an item).

### MENU-14 Text keeps its edit menu
- Method: dispatch a `contextmenu` event (`cancelable`, `bubbles`, `composed`) on each `input`, `textarea` and
  `[contenteditable]`, and on a selected text range in a T. Fail: `defaultPrevented` is true.

## Feedback

### FEED-07 Progress says what it is doing
- Scope: visible `ui-spinner`; T. Fail: a spinner whose `label` is empty or `Loading`; a T whose whole visible text is
  `Loading`, `Loading…` or `Loading...`.

### FEED-09 Never a blank screen
- Method: 200 ms after a view is shown (navigation, column change or window open). Fail: its content root contains no
  T, no visible `ui-spinner` and no visible `ui-empty-state`.

### FEED-10 Content arrives without a jump
- Method: `PerformanceObserver({ type: 'layout-shift', buffered: true })` from view start for 3 s with no input. Fail:
  the sum of `value` for entries with `hadRecentInput === false` > 0.1.

### FEED-17 One banner per window
- Scope: each window. Fail: more than one visible `ui-banner`.

### FEED-20 Feedback is announced
- Method: a `MutationObserver` from load. For each T added, or whose text changes, after load inside a `ui-banner`, a
  `ui-section` footer with `footer-tone="error"`, or a shell `Toast`. Fail: neither it nor a `parent()` ancestor has
  `role` `status` or `alert` or an `aria-live` other than `off`.

## Help

### HELP-10 Glyph-only controls name themselves on hover
- Scope: icon-only I. Fail: no non-empty `title` on the element, its K host, or its hit target.

### HELP-11 A tooltip adds, it never carries the essentials
- Scope: elements with a non-empty `title`. Fail: the `title` equals the element's visible text (case-insensitive,
  trimmed).

## Accessibility

### A11Y-01 Every target is at least 44 × 44
- Scope: I except LAYOUT-03's rows and `a[href]` inside a T with other text (a link in running text). Fail: hit box
  width < 43.5 or height < 43.5.

### A11Y-02 Targets do not overlap
- Scope: pairs of I, neither inside the other (composed). Fail: their hit boxes intersect by more than 1px in both
  axes.

### A11Y-03 Every control has a name
- Scope: I. Fail: empty accessible name.

### A11Y-04 A glyph-only control has a real name
- Scope: icon-only I with a non-empty name. Fail: the name matches
  `/^(button|icon|image|img|svg|graphic|\W+|[a-z0-9]+(\.[a-z0-9]+)+)$/i`.

### A11Y-05 The name contains the visible label
- Scope: I with visible text. Fail: the accessible name (lowercased) does not contain the visible text (lowercased);
  or the name matches `/\b(button|link|switch|checkbox)$/i`.

### A11Y-06 What can be clicked is a control
- Scope: visible A with `cs.cursor === 'pointer'`. Fail: it is not I, not inside I, and not a `label` for a control.

### A11Y-07 State is exposed, not only drawn
- Scope: (a) elements with `aria-haspopup` while their menu or popover is open; (b) `ui-row` hosts and elements with
  role `option` whose `.face` (or own) background equals `--tint` or `--tint-fill`. Fail: (a) `aria-expanded` is not
  `true`; (b) `aria-selected` is not `true`.

### A11Y-08 Decorative images are hidden, meaningful ones described
- Scope: visible `img`; visible `svg` not inside `ui-icon`. Fail: an `img` without an `alt` attribute; an `svg` with no
  accessible name, not `aria-hidden="true"`, and not inside an I.

### A11Y-09 Reading order is visual order
- Scope: A. Fail: `cs.order` ≠ `0` on an element that is or contains an I; or `cs.flexDirection` is `row-reverse` or
  `column-reverse` on an element that contains an I.

### A11Y-10 The document says its language
- Fail: `document.documentElement.lang.trim() === ''`.

### A11Y-11 Focus is always visible
- Method: from the top of the page, press Tab until focus returns to the start (at most 300 stops). After each press,
  take the deepest focused element F (through shadow roots), its K host H if any, and, for a row, the `.face` part.
  Fail: none of F, H, the face has `outline-style` other than `none` with `outline-width` ≥ 2px and `outline-color`
  equal to `--focus-ring` or `--label`; or the outline rect (border box grown by `outline-offset` + `outline-width`) is
  cut by more than 1px by a `parent()` ancestor with `overflow` other than `visible`.

### A11Y-12 No positive tabindex
- Scope: all elements (walk). Fail: `el.tabIndex > 0`.

### A11Y-14 No keyboard traps
- Method: for each I not inside a modal dialog, focus it and press Tab 100 times, recording the deepest focused
  elements. Fail: the recorded elements all lie inside one element R (their lowest common `parent()` ancestor other than
  the root), and the page has an I outside R.

### A11Y-15 Under Reduce Motion nothing moves
- Scope: reduced-motion run; all elements (walk), excluding `ui-spinner` and its shadow content and the shell's
  `ActivityIndicator`. Fail: any comma-separated value of `transitionDuration` or `animationDuration` above 0s; an
  entry of `document.getAnimations()` or a shadow root's `getAnimations()` whose `effect.getTiming().duration` > 0 and
  whose target is not excluded; or `cs(document.documentElement).scrollBehavior === 'smooth'` or a scroller's.

### A11Y-16 The largest Text Size still fits
- Scope: large-text run. Fail: a TYPE-26, TYPE-27 or TYPE-28 failure that does not occur in the default run.

### A11Y-18 Hover is never required
- Method: for each I with `opacity` 0 or `visibility: hidden` at rest, hover its parent (CDP pointer move) and record;
  then move the pointer away and focus it. Fail: it becomes visible on hover but not on focus.

## Motion and materials

### MOTION-01 Blur is for what floats
- Scope: A inside the window content root, not inside a bar, popover, menu or modal dialog. Fail:
  `cs.backdropFilter !== 'none'`.

### MOTION-02 Each surface has its material
- Scope: a bar after its scroller is scrolled by 100px; open alerts, popovers and menus (their outermost element with a
  non-transparent background). Fail: the bar's background is not `--material-chrome`; the others' is not
  `--material-thick`.

### MOTION-03 Materials come from the tokens
- Scope: A. Fail (a): `cs.backdropFilter` is not `none` and not equal to the resolved `--material-blur`; (b) a
  `background-color` with 0 < A < 1 that equals no `--material-*`, `--fill*`, `--separator*` or `--dimming` value;
  (c) in the reduced-transparency run, any `backdropFilter` other than `none`, or a bar, popover, menu or modal
  dialog surface whose background alpha is below 1.

### MOTION-04 Bars are clear until content scrolls under them
- Scope: bars, with their scroller at `scrollTop` 0. Fail: the bar's `background-color` alpha is 1.

### MOTION-05 A dim is only a dim
- Scope: open modal dialogs and shell scrims, outside `Spotlight`. Fail: `cs(dialog, '::backdrop').backdropFilter` or
  the scrim's `backdropFilter` is not `none`.

### MOTION-06 Durations are the tokens
- Scope: A. Fail: a value in `transitionDuration` or `animationDuration` (comma-separated) not in {0s, 0.18s, 0.3s,
  0.45s}, for a property actually transitioned (`transitionProperty` not `none`) or a named animation.

### MOTION-07 Movement eases like iOS
- Scope: A. For each index i of `transitionProperty` that is `all`, `transform`, `translate`, `scale`, `rotate`,
  `width`, `height`, `top`, `left`, `right`, `bottom` or `inset`, take `transitionTimingFunction[i]`. Fail: not
  `cubic-bezier(0.32, 0.72, 0, 1)` and not `cubic-bezier(0.34, 1.36, 0.64, 1)`.

### MOTION-08 Nothing loops but progress
- Scope: all elements (walk) with `animationIterationCount` containing `infinite`. Fail: not inside `ui-spinner` or
  `ActivityIndicator`, no `role` `progressbar`/`status` on it or an ancestor, and its keyframes
  (`el.getAnimations()[i].effect.getKeyframes()`) set a property other than `transform` with only `rotate(…)` values.

## Input

### INPUT-01 Controls show the pointer, text shows the I-beam
- Scope: I other than `input` (text types), `textarea`, `[contenteditable]`, `select`. Fail: `cs.cursor` is `auto` or
  `default` (for a K host, read the inner target's cursor); any element whose `cursor` starts with `url(`.

### INPUT-02 Hover highlights, it doesn't decorate
- Method: for each I inside a list or bar, read `cs.transform`, hover it, read again. Fail: the hovered matrix has a
  scale above 1.

### INPUT-03 Bar targets touch each other
- Scope: each bar's I sorted by left edge. Fail: two neighbours whose centres are less than 44px apart have a
  horizontal gap between hit boxes greater than 1px.

### INPUT-07 One tab stop per group, arrows inside
- Scope: elements with role `listbox`, `radiogroup`, `menu`, `grid`, `tablist`, `tree`, and `ui-list` hosts. Fail: more
  than one descendant (composed) with `tabIndex === 0` that is focusable.

### INPUT-09 Escape closes only the top layer
- Method: in a sheet, open a menu or popover; press Escape once. Fail: the sheet is closed after 300 ms.

### INPUT-10 Focus follows the view
- Method: in a collapsed `ui-split-view`, focus a row in the front column and press Enter. Fail: after 300 ms the
  deepest focused element is `body`, or is not inside the slot that is now `front`.

### INPUT-16 No scroller inside a scroller on the same axis
- Scope: A with `overflowY` `auto`/`scroll` and `scrollHeight > clientHeight + 1`. Fail: a `parent()` ancestor (below
  the root scroller) also has `overflowY` `auto`/`scroll` and `scrollHeight > clientHeight + 1`. The same for x.
