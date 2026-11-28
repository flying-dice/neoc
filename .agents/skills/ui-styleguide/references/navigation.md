# Navigation

iPadOS apps move through a hierarchy with three pieces: a sidebar (or a list) to pick from, a navigation bar that
says where you are and holds the view's actions, and a back button to return. In vos those are `ui-split-view` with
a `ui-list variant="sidebar"` (an app), `SplitViewTemplate` and `NavigationBar` (the shell). There is no tab bar
element: an iPadOS 16–17 tab bar sits at the bottom of the window with each tab's glyph beside its label, and one is
added to the kit before any app shows one.

## Where bar items sit

| Item | Position | Look |
| --- | --- | --- |
| Back (`‹` + the previous view's title) | leading, first | `--tint`, Body 400 |
| Show/hide sidebar | leading | glyph |
| Cancel (in a sheet or edit flow) | leading | text, 400 |
| Title | centre (inline) or large under the bar | Headline / Large Title 700 |
| Edit (toggles to Done) | trailing | text |
| Add (`plus`) | trailing | glyph |
| More (`ellipsis.circle`, a pull-down) | trailing, last | glyph |
| Done, Save, Add (confirming a sheet) | trailing, last | text, 600 |
| Search field | under the title of the column it filters, or trailing for window-wide search | `SearchField` |

---

## Structure

### NAV-01 Hierarchy goes sidebar, list, detail
- **Rule:** Top-level areas or collections are a sidebar; when the hierarchy is deeper than two levels, a middle
  content column (`ui-split-view` `content` slot) lists the children; the detail shows the chosen item. No more than
  two levels inside a sidebar (use disclosure rows for the second).
- **Why (iPadOS):** Sidebars; Split views.
- **Check:** `look`, `source` (`ui-split-view`).
- **Wrong / Right:** a sidebar four levels deep / folders in the sidebar, their notes in the content column.

### NAV-02 The path stays highlighted
- **Rule:** While the split view shows more than one column, each column that leads to the detail keeps its chosen
  row selected (`selection="single"` and `selected` on that row).
- **Why (iPadOS):** Split views.
- **Check:** `auto`: in a `ui-split-view` that is not `collapsed`, a sidebar or content `ui-list` with no `selected` row
  while the detail shows an item.
- **Wrong / Right:** the sidebar loses its highlight when a note opens / "Work" stays selected.

### NAV-03 Collapsed, the columns become a stack
- **Rule:** In a collapsed split view the sidebar is a list; choosing a row shows the next column; the next column's
  bar has a back button to the previous one. The app sets `column` on choose and on back.
- **Why (iPadOS):** Split views; Layout.
- **Check:** `interact` at a window narrower than 700px.
- **Wrong / Right:** a narrow window that shows only the detail with no way back / a back button to `Folders`.

### NAV-04 Selecting navigates in place
- **Rule:** Choosing a sidebar or list row shows its content in the next column of the same window; it never opens a
  new window, a sheet or a different app area without being asked.
- **Why (iPadOS):** Windows; Sidebars.
- **Check:** `interact`.
- **Wrong / Right:** each container opens in a new window / it shows in the detail column, with "Open in New Window" in
  its context menu.

### NAV-05 Sidebar rows look like sidebar rows
- **Rule:** A sidebar is `ui-list variant="sidebar"`: rows with a glyph and a title, the selected row a tint-filled
  rounded pill with white text. Use one icon style per sidebar (tint glyphs, or coloured tiles as in Settings).
- **Why (iPadOS):** Sidebars.
- **Check:** `source`, `look`.
- **Wrong / Right:** an inset-grouped list used as a sidebar / `variant="sidebar"`.

### NAV-06 No floating action buttons
- **Rule:** Actions live in the navigation bar (or a toolbar); no round button floats over content in a window's
  corner.
- **Why (iPadOS):** Toolbars.
- **Check:** `auto`: a `position: fixed|absolute|sticky` element that is a button, not inside a bar, popover or menu,
  whose rect is within 48px of the window content's bottom or trailing edge and has a `--radius-pill` or 50% radius.
- **Wrong / Right:** a blue `+` circle at the bottom right / a `plus` item at the bar's trailing end.

## Navigation bar

### NAV-07 Every view has a bar with a title
- **Rule:** Each view in a window, and each sheet, starts with a bar whose title says where the person is: a short
  noun phrase, under about 15 characters, not the app's name on a detail view. A document whose first line names it
  (an open note) may leave the title empty, as Notes does. The shell uses `NavigationBar`; the kit has no bar element
  yet, so an app's bar is a `header` holding an `h1` (a gap to close in the kit).
- **Why (iPadOS):** Toolbars ("keep the title under 15 characters"; "If titling a toolbar seems redundant, you can
  leave the title area empty").
- **Check:** `look`; `auto`: a bar title longer than 24 characters is flagged.
- **Wrong / Right:** `Containers App — Container Details View` / `nginx`

### NAV-08 Large titles only at the root
- **Rule:** A large title (`NavigationBar large`) is for the root of a column's stack (a sidebar, a top-level list),
  and it collapses into the inline title as the content scrolls. Pushed views and sheets use the inline title. In a
  collapsed split view each column keeps its own title: a list's large title stays large under its back button.
- **Why (iPadOS):** Toolbars; Apple's Settings, and Mail and Notes in Slide Over.
- **Check:** `look`, `interact` (scroll).
- **Wrong / Right:** a large title on a detail three levels in / an inline `General` title there.

### NAV-09 The back button names where it goes
- **Rule:** A pushed view's back button is at the leading edge: the chevron and the previous view's title (`‹
  Folders`), falling back to `Back` only when the title does not fit. Never a custom arrow, never on a root view, never
  in a modal.
- **Why (iPadOS):** Toolbars.
- **Check:** `look` (compare the label with the title in the previous view's picture); `source` (`backLabel`, or an
  app's back button text, is bound to the previous view's title, not written as a literal).
- **Wrong / Right:** `← Return` / `‹ Folders`

### NAV-10 Bar items are borderless
- **Rule:** Bar items are plain tint text or glyphs (`ui-button variant="plain"`, `IconButton`), without fills,
  borders or circles around glyphs.
- **Why (iPadOS):** Toolbars.
- **Check:** `auto`: a `ui-button` in a navigation bar or toolbar whose `variant` is not `plain`, or a bar button with a
  visible background or border at rest.
- **Wrong / Right:** a filled blue `Add` in the bar / a plain `plus` glyph.

### NAV-11 Cancel leads, the confirming action trails
- **Rule:** In a sheet or an edit flow, `Cancel` is the leading bar item and the confirming action (`Done`, `Save`,
  `Add`) is the trailing one; never the other way round, never both on one side.
- **Why (iPadOS):** Toolbars; Sheets.
- **Check:** `auto`: in one bar, a button named `Cancel` whose rect is to the right of a button named `Done`, `Save`,
  `Add` or `Send`.
- **Wrong / Right:** `Done … Cancel` / `Cancel … Done`

### NAV-12 Edit and Add sit trailing
- **Rule:** `Edit` (which becomes `Done` while editing) and `Add` (`plus`) are trailing bar items of the view whose
  content they change; More (`ellipsis.circle`) is the last trailing item.
- **Why (iPadOS):** Toolbars.
- **Check:** `auto`: an `Edit` or `plus` bar item placed in the leading group; `look`.
- **Wrong / Right:** `+` beside the back button / `+` at the trailing end.

### NAV-13 Few items, grouped
- **Rule:** A bar holds at most three groups and only the view's main actions; the rest go in a More pull-down. A text
  item is never directly beside another text item or a glyph without space between them. As the window narrows, the
  less important items move into More; the back button, Cancel, the confirming action and the trailing items stay.
- **Why (iPadOS):** Toolbars ("aim for a maximum of three" groups; "define which items move to the overflow menu as
  the toolbar becomes narrower"; trailing items "remain visible at all window sizes").
- **Check:** `look`; `auto`: more than five interactive items in one bar.
- **Wrong / Right:** eight glyphs in a row / three glyphs and `…`.

## Search

### NAV-14 Search sits by what it searches
- **Rule:** A search field that filters a list sits at the top of that list's column, under its title (the
  `NavigationBar` accessory); one that searches the whole window sits at the trailing end of the bar. In a collapsed
  layout it stays above the list it filters.
- **Why (iPadOS):** Search fields.
- **Check:** `look`.
- **Wrong / Right:** a search field at the bottom of a sidebar / at its top, under `Containers`.

### NAV-15 Search filters as you type
- **Rule:** Results update with each keystroke (debounced if the search is remote), without a Search button;
  Escape clears the field, and an empty result shows a `No Results` empty state.
- **Why (iPadOS):** Search fields.
- **Check:** `interact`.
- **Wrong / Right:** a field with a `Go` button / results that narrow as you type.

### NAV-16 Segments switch views, not app areas
- **Rule:** A segmented control in a bar switches between views of the same content (`List`/`Grid`, `Day`/`Week`);
  top-level areas of an app are sidebar rows, never segments or hand-made tabs.
- **Why (iPadOS):** Tab bars; Segmented controls.
- **Check:** `look`, `source` (no hand-rolled tab bar).
- **Wrong / Right:** `Containers | Images | Volumes` segments as the app's sections / three sidebar rows.
