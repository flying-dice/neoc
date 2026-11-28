# Rule index

Every rule of the styleguide, 344 in all (182 with an `auto` check, specified in
[auto-checks.md](auto-checks.md)). Walk it top to bottom for a review; for an answer key, copy the table and
fill the Result column (`pass`, `fail: <where>`, `n/a`).

Check types: `auto` a script tests it in the browser; `source` read the code; `look` look at the screen in both
themes; `interact` operate it (keyboard, pointer, resize, settings).

| File | Rules |
| --- | --- |
| [Typography](typography.md) | 50 |
| [Writing](writing.md) | 34 |
| [Color](color.md) | 21 |
| [Layout](layout.md) | 19 |
| [Navigation](navigation.md) | 16 |
| [Controls](controls.md) | 28 |
| [Forms](forms.md) | 29 |
| [Lists](lists.md) | 17 |
| [Modals](modals.md) | 23 |
| [Menus](menus.md) | 15 |
| [Feedback](feedback.md) | 23 |
| [Help](help.md) | 13 |
| [Accessibility](accessibility.md) | 20 |
| [Motion and materials](motion-and-materials.md) | 12 |
| [Input](input.md) | 24 |

| ID | Rule | Check | File | Result |
| --- | --- | --- | --- | --- |
| TYPE-01 | Every font size is a ramp size | `auto` | [Typography](typography.md) | |
| TYPE-02 | Text follows the Text Size setting | `auto` | [Typography](typography.md) | |
| TYPE-03 | Line height is the style's leading | `auto` | [Typography](typography.md) | |
| TYPE-04 | The smallest text is Caption 2 | `auto` | [Typography](typography.md) | |
| TYPE-05 | Glyphs that carry meaning grow with the text | `look`, `interact` | [Typography](typography.md) | |
| TYPE-06 | At large sizes the layout changes, not the hierarchy | `look`, `interact` | [Typography](typography.md) | |
| TYPE-07 | A large title is Large Title bold | `auto`, `look` | [Typography](typography.md) | |
| TYPE-08 | An inline bar title is Headline | `auto` | [Typography](typography.md) | |
| TYPE-09 | A row title is Body | `auto`, `look` | [Typography](typography.md) | |
| TYPE-10 | A row subtitle is Subheadline, secondary | `source`, `look` | [Typography](typography.md) | |
| TYPE-11 | A row's trailing value is Body, secondary | `source`, `look` | [Typography](typography.md) | |
| TYPE-12 | A grouped section header is Footnote caps, secondary | `source`, `look` | [Typography](typography.md) | |
| TYPE-13 | A sidebar section header is Headline | `source`, `look` | [Typography](typography.md) | |
| TYPE-14 | A section footer is Footnote, secondary | `source`, `look` | [Typography](typography.md) | |
| TYPE-15 | Running text is Body | `look` | [Typography](typography.md) | |
| TYPE-16 | Timestamps and metadata are quieter than the title | `auto`, `look` | [Typography](typography.md) | |
| TYPE-17 | Buttons take the kit's type | `source` | [Typography](typography.md) | |
| TYPE-18 | The confirming bar button is semibold | `auto`, `look` | [Typography](typography.md) | |
| TYPE-19 | A count badge is Caption 1 semibold | `look` | [Typography](typography.md) | |
| TYPE-20 | Only four weights exist | `auto` | [Typography](typography.md) | |
| TYPE-21 | Emphasis is one step, by size class | `auto` | [Typography](typography.md) | |
| TYPE-22 | Emphasis is weight or colour, not size | `look` | [Typography](typography.md) | |
| TYPE-23 | Text is in the vos font | `auto` | [Typography](typography.md) | |
| TYPE-24 | Monospace only for code and data | `auto`, `source`, `look` | [Typography](typography.md) | |
| TYPE-25 | Letter spacing is the style's tracking | `auto` | [Typography](typography.md) | |
| TYPE-26 | Clipped text ends in an ellipsis | `auto` | [Typography](typography.md) | |
| TYPE-27 | Text never spills out of its box | `auto` | [Typography](typography.md) | |
| TYPE-28 | Text never overlaps text or controls | `auto` | [Typography](typography.md) | |
| TYPE-29 | Tail truncation for names and prose | `look` | [Typography](typography.md) | |
| TYPE-30 | Middle truncation where the end tells items apart | `look` | [Typography](typography.md) | |
| TYPE-31 | What is cut can be read in full | `source`, `interact` | [Typography](typography.md) | |
| TYPE-32 | Messages wrap, never truncate | `auto`, `look` | [Typography](typography.md) | |
| TYPE-33 | Text aligns to the leading edge | `auto`, `look` | [Typography](typography.md) | |
| TYPE-34 | Nothing is justified | `auto` | [Typography](typography.md) | |
| TYPE-35 | Numbers that line up are tabular | `auto` | [Typography](typography.md) | |
| TYPE-36 | Body text keeps a readable measure | `auto` | [Typography](typography.md) | |
| TYPE-37 | Uppercase only by transform, only for short labels | `auto` | [Typography](typography.md) | |
| TYPE-38 | Caps are never typed | `source` | [Typography](typography.md) | |
| TYPE-39 | No small caps | `auto` | [Typography](typography.md) | |
| TYPE-40 | Italic only in content | `auto` | [Typography](typography.md) | |
| TYPE-41 | Underline means a link | `auto` | [Typography](typography.md) | |
| TYPE-42 | The label level matches the text's importance | `look` | [Typography](typography.md) | |
| TYPE-43 | Tertiary label is never for reading | `auto` | [Typography](typography.md) | |
| TYPE-44 | Secondary text is secondary by colour, not opacity | `auto`, `source` | [Typography](typography.md) | |
| TYPE-45 | Text on an image sits on something solid | `look` | [Typography](typography.md) | |
| TYPE-46 | Text on a material uses label colours | `source`, `look` | [Typography](typography.md) | |
| TYPE-47 | Heading levels follow the visual order | `auto` | [Typography](typography.md) | |
| TYPE-48 | Text people copy is selectable, chrome is not | `interact` | [Typography](typography.md) | |
| TYPE-49 | One role, one style | `look` | [Typography](typography.md) | |
| TYPE-50 | Text side by side shares a baseline | `look` | [Typography](typography.md) | |
| WRITE-01 | Titles are title case | `auto` | [Writing](writing.md) | |
| WRITE-02 | Buttons and menu items are title case | `auto` | [Writing](writing.md) | |
| WRITE-03 | Row titles and field labels are title case | `auto`, `look` | [Writing](writing.md) | |
| WRITE-04 | Section headers and column headings are title case | `source` | [Writing](writing.md) | |
| WRITE-05 | Messages are sentence case | `auto` | [Writing](writing.md) | |
| WRITE-06 | An alert title's case follows its grammar | `source`, `look` | [Writing](writing.md) | |
| WRITE-07 | Say it in the fewest plain words | `look` | [Writing](writing.md) | |
| WRITE-08 | Neutral and calm, never cute | `auto` | [Writing](writing.md) | |
| WRITE-09 | No "we", sparing "your" | `auto`, `look` | [Writing](writing.md) | |
| WRITE-10 | Device-neutral verbs | `auto` | [Writing](writing.md) | |
| WRITE-11 | Link text says where it goes | `auto` | [Writing](writing.md) | |
| WRITE-12 | A button says what it does, with a verb | `auto`, `look` | [Writing](writing.md) | |
| WRITE-13 | No OK where a verb fits | `auto` | [Writing](writing.md) | |
| WRITE-14 | Cancel is always "Cancel" | `auto` | [Writing](writing.md) | |
| WRITE-15 | Done ends, Cancel abandons | `interact` | [Writing](writing.md) | |
| WRITE-16 | Delete destroys, Remove takes out | `source`, `interact` | [Writing](writing.md) | |
| WRITE-17 | An ellipsis means more input follows | `interact` | [Writing](writing.md) | |
| WRITE-18 | No ending punctuation on labels | `auto` | [Writing](writing.md) | |
| WRITE-19 | Typographic punctuation | `auto` | [Writing](writing.md) | |
| WRITE-20 | Quote a name the person gave | `look` | [Writing](writing.md) | |
| WRITE-21 | Numbers are numerals, formatted for the locale | `auto`, `source` | [Writing](writing.md) | |
| WRITE-22 | Units are standard and spaced | `auto` | [Writing](writing.md) | |
| WRITE-23 | Dates read like a person would say them | `auto` | [Writing](writing.md) | |
| WRITE-24 | An error says what happened and what to do | `look` | [Writing](writing.md) | |
| WRITE-25 | Errors don't blame | `auto` | [Writing](writing.md) | |
| WRITE-26 | Language includes everyone | `auto`, `look` | [Writing](writing.md) | |
| WRITE-27 | No developer text in the interface | `auto` | [Writing](writing.md) | |
| WRITE-28 | No jargon the person did not bring | `look` | [Writing](writing.md) | |
| WRITE-29 | An alert title says the situation | `auto` | [Writing](writing.md) | |
| WRITE-30 | An alert message adds something or is left out | `look` | [Writing](writing.md) | |
| WRITE-31 | A placeholder is a hint, not the label | `auto` | [Writing](writing.md) | |
| WRITE-32 | An empty state says what is missing and what to do | `look` | [Writing](writing.md) | |
| WRITE-33 | A setting says what On does | `look` | [Writing](writing.md) | |
| WRITE-34 | One term for one thing | `source`, `look` | [Writing](writing.md) | |
| COLOR-01 | No colour literals in app code | `source` | [Color](color.md) | |
| COLOR-02 | Every colour on screen is a token value | `auto` | [Color](color.md) | |
| COLOR-03 | Text colours come from the label set | `auto` | [Color](color.md) | |
| COLOR-04 | Semantic tokens keep their meaning | `auto` | [Color](color.md) | |
| COLOR-05 | Red and blue are the accessible variants | `auto` | [Color](color.md) | |
| COLOR-06 | Tint means it acts | `auto` | [Color](color.md) | |
| COLOR-07 | The tint is the person's accent | `source` | [Color](color.md) | |
| COLOR-08 | Red is for destroying and for errors | `auto` | [Color](color.md) | |
| COLOR-09 | What destroys is red | `auto`, `source` | [Color](color.md) | |
| COLOR-10 | Grouped lists sit on a grouped background | `auto` | [Color](color.md) | |
| COLOR-11 | Surfaces above the desktop are elevated | `auto` | [Color](color.md) | |
| COLOR-12 | Lines are separator hairlines | `auto` | [Color](color.md) | |
| COLOR-13 | Fills are for small shapes | `auto` | [Color](color.md) | |
| COLOR-14 | Text holds 4.5:1 | `auto` | [Color](color.md) | |
| COLOR-15 | Large text holds 3:1 | `auto` | [Color](color.md) | |
| COLOR-16 | Glyphs and control edges hold 3:1 | `auto` | [Color](color.md) | |
| COLOR-17 | Placeholders hold 4.5:1 | `auto` | [Color](color.md) | |
| COLOR-18 | Colour is never the only signal | `auto`, `look` | [Color](color.md) | |
| COLOR-19 | Both themes, from one declaration | `source` | [Color](color.md) | |
| COLOR-20 | An app never forces a theme on its chrome | `auto` | [Color](color.md) | |
| COLOR-21 | Nothing relies on Increase Contrast | `source` | [Color](color.md) | |
| LAYOUT-01 | Text keeps off the window's edge | `auto` | [Layout](layout.md) | |
| LAYOUT-02 | Spacing is on the 4-point grid | `auto` | [Layout](layout.md) | |
| LAYOUT-03 | Rows are at least 44 high | `auto`, `interact` | [Layout](layout.md) | |
| LAYOUT-04 | Row content is inset 16 | `auto` | [Layout](layout.md) | |
| LAYOUT-05 | Groups are separated by space, rows by hairlines | `look` | [Layout](layout.md) | |
| LAYOUT-06 | Things that belong together align | `auto`, `look` | [Layout](layout.md) | |
| LAYOUT-07 | The most important thing is top and leading | `look` | [Layout](layout.md) | |
| LAYOUT-08 | Nothing hides under window chrome | `auto` | [Layout](layout.md) | |
| LAYOUT-09 | Lay out for the window, not the screen | `source` | [Layout](layout.md) | |
| LAYOUT-10 | One shared compact width | `source` | [Layout](layout.md) | |
| LAYOUT-11 | Every function at every width | `interact` | [Layout](layout.md) | |
| LAYOUT-12 | The window holds together at its smallest | `auto` | [Layout](layout.md) | |
| LAYOUT-13 | Columns are the split view's | `source` | [Layout](layout.md) | |
| LAYOUT-14 | Radii come from the tokens | `auto` | [Layout](layout.md) | |
| LAYOUT-15 | Nested corners are concentric | `auto` | [Layout](layout.md) | |
| LAYOUT-16 | Shadows come from the tokens | `auto` | [Layout](layout.md) | |
| LAYOUT-17 | No box inside a box | `auto` | [Layout](layout.md) | |
| LAYOUT-18 | Images keep their shape | `auto` | [Layout](layout.md) | |
| LAYOUT-19 | Images are sharp | `auto` | [Layout](layout.md) | |
| NAV-01 | Hierarchy goes sidebar, list, detail | `source`, `look` | [Navigation](navigation.md) | |
| NAV-02 | The path stays highlighted | `auto` | [Navigation](navigation.md) | |
| NAV-03 | Collapsed, the columns become a stack | `interact` | [Navigation](navigation.md) | |
| NAV-04 | Selecting navigates in place | `interact` | [Navigation](navigation.md) | |
| NAV-05 | Sidebar rows look like sidebar rows | `source`, `look` | [Navigation](navigation.md) | |
| NAV-06 | No floating action buttons | `auto` | [Navigation](navigation.md) | |
| NAV-07 | Every view has a bar with a title | `auto`, `look` | [Navigation](navigation.md) | |
| NAV-08 | Large titles only at the root | `look`, `interact` | [Navigation](navigation.md) | |
| NAV-09 | The back button names where it goes | `source`, `look` | [Navigation](navigation.md) | |
| NAV-10 | Bar items are borderless | `auto` | [Navigation](navigation.md) | |
| NAV-11 | Cancel leads, the confirming action trails | `auto` | [Navigation](navigation.md) | |
| NAV-12 | Edit and Add sit trailing | `auto`, `look` | [Navigation](navigation.md) | |
| NAV-13 | Few items, grouped | `auto`, `look` | [Navigation](navigation.md) | |
| NAV-14 | Search sits by what it searches | `look` | [Navigation](navigation.md) | |
| NAV-15 | Search filters as you type | `interact` | [Navigation](navigation.md) | |
| NAV-16 | Segments switch views, not app areas | `source`, `look` | [Navigation](navigation.md) | |
| CTRL-01 | Buttons are kit buttons | `auto`, `source` | [Controls](controls.md) | |
| CTRL-02 | One prominent button per view | `auto` | [Controls](controls.md) | |
| CTRL-03 | Style, not size, ranks a set | `auto` | [Controls](controls.md) | |
| CTRL-04 | A destructive button is never the prominent one | `auto` | [Controls](controls.md) | |
| CTRL-05 | Sizes have places | `look` | [Controls](controls.md) | |
| CTRL-06 | A slow button says it is working | `interact` | [Controls](controls.md) | |
| CTRL-07 | A switch lives in a list row | `auto` | [Controls](controls.md) | |
| CTRL-08 | A switch has no On/Off text | `auto` | [Controls](controls.md) | |
| CTRL-09 | A switch acts at once | `interact` | [Controls](controls.md) | |
| CTRL-10 | A toggle button changes its fill | `auto` | [Controls](controls.md) | |
| CTRL-11 | No square checkboxes | `auto` | [Controls](controls.md) | |
| CTRL-12 | Two to five segments | `auto` | [Controls](controls.md) | |
| CTRL-13 | Segments select, they don't act | `interact` | [Controls](controls.md) | |
| CTRL-14 | Segment labels are short nouns | `look` | [Controls](controls.md) | |
| CTRL-15 | One of many is a picker | `auto` | [Controls](controls.md) | |
| CTRL-16 | A picker always shows a choice | `auto` | [Controls](controls.md) | |
| CTRL-17 | Pop-up for values, pull-down for actions | `interact` | [Controls](controls.md) | |
| CTRL-18 | A pull-down earns its menu | `interact` | [Controls](controls.md) | |
| CTRL-19 | Sliders run minimum to maximum, leading to trailing | `interact` | [Controls](controls.md) | |
| CTRL-20 | A stepper sits by the value it changes | `look` | [Controls](controls.md) | |
| CTRL-21 | Disclosure says what it reveals | `look`, `interact` | [Controls](controls.md) | |
| CTRL-22 | Page dots only for flat pages | `look` | [Controls](controls.md) | |
| CTRL-23 | Read-only text is a label | `auto` | [Controls](controls.md) | |
| CTRL-24 | Links go somewhere, buttons do something | `auto`, `source` | [Controls](controls.md) | |
| CTRL-25 | A meter has a name and a number | `auto`, `look` | [Controls](controls.md) | |
| CTRL-26 | Glyphs come from the icon set | `auto` | [Controls](controls.md) | |
| CTRL-27 | Standard actions use standard glyphs | `source`, `look` | [Controls](controls.md) | |
| CTRL-28 | Glyph weight matches the text | `auto` | [Controls](controls.md) | |
| FORM-01 | A form is a list of sections, a row per value | `source` | [Forms](forms.md) | |
| FORM-02 | On the window's own background, the list is inset | `auto` | [Forms](forms.md) | |
| FORM-03 | The footer is the form's one voice | `source` | [Forms](forms.md) | |
| FORM-04 | The commit button comes last | `look` | [Forms](forms.md) | |
| FORM-05 | The label leads, the value trails | `auto`, `look` | [Forms](forms.md) | |
| FORM-06 | Related fields share a section | `look` | [Forms](forms.md) | |
| FORM-07 | Offer a choice instead of text | `look` | [Forms](forms.md) | |
| FORM-08 | Fields start with sensible defaults | `look` | [Forms](forms.md) | |
| FORM-09 | The right keyboard and limits | `auto` | [Forms](forms.md) | |
| FORM-10 | Identifiers are not capitalised or corrected | `auto` | [Forms](forms.md) | |
| FORM-11 | Secrets use secure entry | `auto` | [Forms](forms.md) | |
| FORM-12 | Sign-in fields say what they are | `auto` | [Forms](forms.md) | |
| FORM-13 | Clearing a field is one tap | `interact` | [Forms](forms.md) | |
| FORM-14 | Paste and drop always work | `source` | [Forms](forms.md) | |
| FORM-15 | Disabled until there is something to send | `auto`, `interact` | [Forms](forms.md) | |
| FORM-16 | An empty required field is not an error | `interact` | [Forms](forms.md) | |
| FORM-17 | Validation is client state that mirrors the backend | `source` | [Forms](forms.md) | |
| FORM-18 | The field shows it is wrong | `auto` | [Forms](forms.md) | |
| FORM-19 | The sentence goes in the section's footer | `auto`, `source` | [Forms](forms.md) | |
| FORM-20 | One problem at a time, top to bottom | `interact` | [Forms](forms.md) | |
| FORM-21 | The field describes its problem to a screen reader | `auto` | [Forms](forms.md) | |
| FORM-22 | Speak after leaving, then follow each keystroke | `interact` | [Forms](forms.md) | |
| FORM-23 | Say the rule and show a right value | `look` | [Forms](forms.md) | |
| FORM-24 | A refusal from the backend lands on the form | `interact` | [Forms](forms.md) | |
| FORM-25 | A banner is for the window, not for input | `auto` | [Forms](forms.md) | |
| FORM-26 | No alert for input | `interact` | [Forms](forms.md) | |
| FORM-27 | No message outlives its cause | `interact` | [Forms](forms.md) | |
| FORM-28 | Unit-test the validators and the state | `source` | [Forms](forms.md) | |
| FORM-29 | Stories and journeys assert what the person sees | `source` | [Forms](forms.md) | |
| LIST-01 | Lists are kit lists | `auto` | [Lists](lists.md) | |
| LIST-02 | The list style fits the content | `look` | [Lists](lists.md) | |
| LIST-03 | Use the row's properties | `source` | [Lists](lists.md) | |
| LIST-04 | A chevron means it navigates | `auto` | [Lists](lists.md) | |
| LIST-05 | Rows that drill in show it | `interact` | [Lists](lists.md) | |
| LIST-06 | An action row is tinted, not chevroned | `auto`, `source` | [Lists](lists.md) | |
| LIST-07 | One control at a row's end | `auto` | [Lists](lists.md) | |
| LIST-08 | The info button only informs | `interact` | [Lists](lists.md) | |
| LIST-09 | A row shows a title, not the content | `look` | [Lists](lists.md) | |
| LIST-10 | Headers name groups, footers explain them | `look` | [Lists](lists.md) | |
| LIST-11 | The kit draws the separators | `auto` | [Lists](lists.md) | |
| LIST-12 | Navigation highlights, choice checks | `look`, `interact` | [Lists](lists.md) | |
| LIST-13 | Multiple selection is a mode | `interact` | [Lists](lists.md) | |
| LIST-14 | Every swipe action is also in the menu | `interact` | [Lists](lists.md) | |
| LIST-15 | Delete is the trailing, red swipe action | `interact` | [Lists](lists.md) | |
| LIST-16 | An empty list says so | `auto` | [Lists](lists.md) | |
| LIST-17 | Long lists scroll, they don't page | `look` | [Lists](lists.md) | |
| MODAL-01 | One modal at a time | `auto` | [Modals](modals.md) | |
| MODAL-02 | Modals are kit or shell elements | `auto` | [Modals](modals.md) | |
| MODAL-03 | A modal dims what it blocks | `auto` | [Modals](modals.md) | |
| MODAL-04 | Focus stays inside and comes back | `auto` | [Modals](modals.md) | |
| MODAL-05 | Escape means Cancel | `auto` | [Modals](modals.md) | |
| MODAL-06 | A modal names its task | `auto`, `look` | [Modals](modals.md) | |
| MODAL-07 | No hierarchy inside a modal | `look` | [Modals](modals.md) | |
| MODAL-08 | Alerts are for what matters now | `look`, `interact` | [Modals](modals.md) | |
| MODAL-09 | An alert title fits in two lines | `auto` | [Modals](modals.md) | |
| MODAL-10 | Two buttons side by side, three stacked, never more | `auto` | [Modals](modals.md) | |
| MODAL-11 | Cancel leads, the likely answer trails | `auto` | [Modals](modals.md) | |
| MODAL-12 | A destructive alert has Cancel | `auto` | [Modals](modals.md) | |
| MODAL-13 | The safe likely answer is the default | `auto` | [Modals](modals.md) | |
| MODAL-14 | Choices about the person's own action are an action sheet | `look` | [Modals](modals.md) | |
| MODAL-15 | Destructive first, Cancel last | `look` | [Modals](modals.md) | |
| MODAL-16 | A sheet has Done and Cancel, or Back | `auto` | [Modals](modals.md) | |
| MODAL-17 | Closing with changes asks first | `interact` | [Modals](modals.md) | |
| MODAL-18 | Sheets are for short tasks, centred | `look` | [Modals](modals.md) | |
| MODAL-19 | A popover points at what opened it | `auto` | [Modals](modals.md) | |
| MODAL-20 | A popover closes by itself and keeps the work | `interact` | [Modals](modals.md) | |
| MODAL-21 | Popovers are small and only in regular width | `auto` | [Modals](modals.md) | |
| MODAL-22 | Never warn in a popover | `look` | [Modals](modals.md) | |
| MODAL-23 | A full-window cover is for immersion | `look`, `interact` | [Modals](modals.md) | |
| MENU-01 | Menus are the shell's menu | `auto`, `source` | [Menus](menus.md) | |
| MENU-02 | A context menu opens three ways | `auto`, `interact` | [Menus](menus.md) | |
| MENU-03 | Context menus are everywhere or nowhere | `interact` | [Menus](menus.md) | |
| MENU-04 | Nothing lives only in a context menu | `interact` | [Menus](menus.md) | |
| MENU-05 | Short, with few groups | `auto` | [Menus](menus.md) | |
| MENU-06 | Frequent first, related together | `look` | [Menus](menus.md) | |
| MENU-07 | Destructive items go last | `auto` | [Menus](menus.md) | |
| MENU-08 | Item labels are verbs without articles | `auto` | [Menus](menus.md) | |
| MENU-09 | Glyphs for all items of a group, or none | `auto` | [Menus](menus.md) | |
| MENU-10 | Context menus hide what does not apply | `auto` | [Menus](menus.md) | |
| MENU-11 | No shortcuts in context menus | `auto` | [Menus](menus.md) | |
| MENU-12 | One level of submenu | `auto` | [Menus](menus.md) | |
| MENU-13 | A checkmark shows what is in effect | `look` | [Menus](menus.md) | |
| MENU-14 | Text keeps its edit menu | `auto` | [Menus](menus.md) | |
| MENU-15 | Choosing acts at once | `interact` | [Menus](menus.md) | |
| FEED-01 | Feedback matches its weight | `look` | [Feedback](feedback.md) | |
| FEED-02 | Status sits on what it describes | `look` | [Feedback](feedback.md) | |
| FEED-03 | Known amounts get a determinate indicator | `interact` | [Feedback](feedback.md) | |
| FEED-04 | Spinners are the kit's | `source` | [Feedback](feedback.md) | |
| FEED-05 | Progress moves honestly | `interact` | [Feedback](feedback.md) | |
| FEED-06 | One indicator shape per task | `interact` | [Feedback](feedback.md) | |
| FEED-07 | Progress says what it is doing | `auto` | [Feedback](feedback.md) | |
| FEED-08 | Long work can be stopped | `interact` | [Feedback](feedback.md) | |
| FEED-09 | Never a blank screen | `auto` | [Feedback](feedback.md) | |
| FEED-10 | Content arrives without a jump | `auto` | [Feedback](feedback.md) | |
| FEED-11 | The rest stays usable while loading | `interact` | [Feedback](feedback.md) | |
| FEED-12 | Content refreshes itself | `interact` | [Feedback](feedback.md) | |
| FEED-13 | Empty and loading are different | `interact` | [Feedback](feedback.md) | |
| FEED-14 | An error appears where it happened | `look` | [Feedback](feedback.md) | |
| FEED-15 | A command that cannot run says why | `look` | [Feedback](feedback.md) | |
| FEED-16 | A banner offers the fix | `look`, `interact` | [Feedback](feedback.md) | |
| FEED-17 | One banner per window | `auto` | [Feedback](feedback.md) | |
| FEED-18 | Toasts confirm, they don't warn | `look`, `interact` | [Feedback](feedback.md) | |
| FEED-19 | Undoable deletes offer Undo, not a warning | `interact` | [Feedback](feedback.md) | |
| FEED-20 | Feedback is announced | `auto` | [Feedback](feedback.md) | |
| FEED-21 | Notify only while away | `interact` | [Feedback](feedback.md) | |
| FEED-22 | A notification is short and private | `look` | [Feedback](feedback.md) | |
| FEED-23 | Badges count unread notifications only | `look` | [Feedback](feedback.md) | |
| HELP-01 | The first screen is the app | `look` | [Help](help.md) | |
| HELP-02 | It comes back where it was | `interact` | [Help](help.md) | |
| HELP-03 | It works before it is configured | `interact` | [Help](help.md) | |
| HELP-04 | Ask for access when it is needed, and say why | `interact` | [Help](help.md) | |
| HELP-05 | Onboarding only if it is needed, and brief | `look` | [Help](help.md) | |
| HELP-06 | Onboarding can be skipped and is shown once | `interact` | [Help](help.md) | |
| HELP-07 | Teach the app, not the system | `look` | [Help](help.md) | |
| HELP-08 | Help sits next to what it explains | `look` | [Help](help.md) | |
| HELP-09 | A tip is one or two sentences, once | `interact` | [Help](help.md) | |
| HELP-10 | Glyph-only controls name themselves on hover | `auto` | [Help](help.md) | |
| HELP-11 | A tooltip adds, it never carries the essentials | `auto`, `source`, `look` | [Help](help.md) | |
| HELP-12 | Options live where they act | `look` | [Help](help.md) | |
| HELP-13 | No copies of system settings | `source`, `look` | [Help](help.md) | |
| A11Y-01 | Every target is at least 44 × 44 | `auto` | [Accessibility](accessibility.md) | |
| A11Y-02 | Targets do not overlap | `auto` | [Accessibility](accessibility.md) | |
| A11Y-03 | Every control has a name | `auto` | [Accessibility](accessibility.md) | |
| A11Y-04 | A glyph-only control has a real name | `auto` | [Accessibility](accessibility.md) | |
| A11Y-05 | The name contains the visible label | `auto` | [Accessibility](accessibility.md) | |
| A11Y-06 | What can be clicked is a control | `auto` | [Accessibility](accessibility.md) | |
| A11Y-07 | State is exposed, not only drawn | `auto` | [Accessibility](accessibility.md) | |
| A11Y-08 | Decorative images are hidden, meaningful ones described | `auto` | [Accessibility](accessibility.md) | |
| A11Y-09 | Reading order is visual order | `auto` | [Accessibility](accessibility.md) | |
| A11Y-10 | The document says its language | `auto` | [Accessibility](accessibility.md) | |
| A11Y-11 | Focus is always visible | `auto`, `source` | [Accessibility](accessibility.md) | |
| A11Y-12 | No positive tabindex | `auto` | [Accessibility](accessibility.md) | |
| A11Y-13 | Everything works from the keyboard | `interact` | [Accessibility](accessibility.md) | |
| A11Y-14 | No keyboard traps | `auto` | [Accessibility](accessibility.md) | |
| A11Y-15 | Under Reduce Motion nothing moves | `auto` | [Accessibility](accessibility.md) | |
| A11Y-16 | The largest Text Size still fits | `auto` | [Accessibility](accessibility.md) | |
| A11Y-17 | The page holds at 200% zoom | `interact` | [Accessibility](accessibility.md) | |
| A11Y-18 | Hover is never required | `auto`, `interact` | [Accessibility](accessibility.md) | |
| A11Y-19 | Gestures have plain alternatives | `interact` | [Accessibility](accessibility.md) | |
| A11Y-20 | Nothing needed disappears on a timer | `interact` | [Accessibility](accessibility.md) | |
| MOTION-01 | Blur is for what floats | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-02 | Each surface has its material | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-03 | Materials come from the tokens | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-04 | Bars are clear until content scrolls under them | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-05 | A dim is only a dim | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-06 | Durations are the tokens | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-07 | Movement eases like iOS | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-08 | Nothing loops but progress | `auto` | [Motion and materials](motion-and-materials.md) | |
| MOTION-09 | Frequent actions don't animate | `interact` | [Motion and materials](motion-and-materials.md) | |
| MOTION-10 | Motion follows where things go | `interact` | [Motion and materials](motion-and-materials.md) | |
| MOTION-11 | Nobody waits for an animation | `interact` | [Motion and materials](motion-and-materials.md) | |
| MOTION-12 | Motion is never the message | `look` | [Motion and materials](motion-and-materials.md) | |
| INPUT-01 | Controls show the pointer, text shows the I-beam | `auto` | [Input](input.md) | |
| INPUT-02 | Hover highlights, it doesn't decorate | `auto`, `interact` | [Input](input.md) | |
| INPUT-03 | Bar targets touch each other | `auto` | [Input](input.md) | |
| INPUT-04 | Shortcuts are declared and shown | `source` | [Input](input.md) | |
| INPUT-05 | Standard shortcuts keep their meaning | `source`, `interact` | [Input](input.md) | |
| INPUT-06 | Few custom shortcuts, made with ⌘ | `source` | [Input](input.md) | |
| INPUT-07 | One tab stop per group, arrows inside | `auto` | [Input](input.md) | |
| INPUT-08 | Return does the default thing | `interact` | [Input](input.md) | |
| INPUT-09 | Escape closes only the top layer | `auto` | [Input](input.md) | |
| INPUT-10 | Focus follows the view | `auto` | [Input](input.md) | |
| INPUT-11 | Drag where people expect to | `interact` | [Input](input.md) | |
| INPUT-12 | Destinations say whether they accept | `interact` | [Input](input.md) | |
| INPUT-13 | A failed drop goes back | `interact` | [Input](input.md) | |
| INPUT-14 | Move inside, copy across | `interact` | [Input](input.md) | |
| INPUT-15 | Dropped items stay selected, and a drop can be undone | `interact` | [Input](input.md) | |
| INPUT-16 | No scroller inside a scroller on the same axis | `auto` | [Input](input.md) | |
| INPUT-17 | Scrolling is the system's | `source` | [Input](input.md) | |
| INPUT-18 | Resizing keeps everything | `interact` | [Input](input.md) | |
| INPUT-19 | No window chrome of an app's own | `source`, `look` | [Input](input.md) | |
| INPUT-20 | Items worth their own window can have one | `interact` | [Input](input.md) | |
| INPUT-21 | Work pauses or carries on sensibly in the background | `interact` | [Input](input.md) | |
| INPUT-22 | Edits can be undone, many times | `interact` | [Input](input.md) | |
| INPUT-23 | Undo shows what it undid | `interact` | [Input](input.md) | |
| INPUT-24 | Undo says what it will undo | `look` | [Input](input.md) | |
