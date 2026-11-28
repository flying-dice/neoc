# Interact: what to operate

The rules a picture cannot settle. Do each journey once per app, in the built Storybook or the running app; where you
cannot drive it, read the code that would run (the handlers, the `:hover`/`:focus-visible` styles, the key handling,
the resize behaviour). Write which you did for each journey: a journey only read is weaker evidence, and a handler you
could not follow goes under Not checked. The machine's operate run already did one Tab walk, a hover and a click on
each toggle, and Escape on what was open when the story rendered (checks.md, Runs); the journeys are what it leaves.

## 1. The keyboard alone

Put the pointer away. Tab from the top of the window to its end and back with Shift+Tab, using the arrows inside each
list. Open each menu from the keyboard (Shift+F10 on its item) and each alert and popover from its control: focus goes
inside, Escape closes it as Cancel, and focus comes back to the control that opened it (MODAL-04, MODAL-05: the
machine tried only what was open at render). Return on a row moves focus into the column it shows (INPUT-10); Tab
leaves each editor and terminal (A11Y-14).
- [ ] **A11Y-13** Every task can be finished with the keyboard: open, move, reorder, delete.
- [ ] **MENU-02** Each item with a context menu opens it from the keyboard too.
- [ ] **INPUT-08** Return in a single-line field submits its form; in an alert it takes the bold button; in a text area it starts a new line.
- [ ] **INPUT-05** ⌘Z, ⌘F, ⌘N, ⌘W, ⌘, and Escape do what input.md's table says, and the app takes none of them for something else.
- [ ] **NAV-15** Search narrows the results at each keystroke, with no Search button; Escape clears it; nothing found shows `No Results`.
- [ ] **A11Y-19** Every long press, drag and swipe has a click, a menu item or a key that does the same.
- [ ] **LIST-14** Every swipe action is also in the row's context menu.

## 2. Pointer, then touch

Hover each kind of control with the pointer. Then go over the screen again as a finger would, with no hover at all.
- [ ] **INPUT-02** Hover gives a small control a soft rounded fill; nothing scales, gains a shadow or animates for show.
- [ ] **MOTION-09** Selecting, toggling and hovering change at once, with no movement of the app's own.
- [ ] **A11Y-18** Nothing appears only on hover: what hover shows is also there on focus, and reachable by touch.
- [ ] **TYPE-31** A cut name can be read in full: in its detail, its tooltip and its accessible name.
- [ ] **TYPE-48** Values people would copy (an address, an ID, an error) can be selected; labels and bar items cannot.

## 3. Narrow the window

Select an item, scroll, type into a field, then go from full width to half, to Slide Over (375) and back.
- [ ] **NAV-03** Below 700px the columns become a stack: choosing a row shows the next column, whose back button returns to the one before.
- [ ] **INPUT-18** The selection, the scroll position, the typed text and the column in front survive every resize.
- [ ] **LAYOUT-11** Everything that can be done at full width can be done in Slide Over (moved into More or behind navigation).
- [ ] **TYPE-06** At ×1.25 in Slide Over, side-by-side texts stack instead of truncating, and the hierarchy stays.
- [ ] **TYPE-05** At ×1.25 the glyphs that carry meaning grew with the text.
- [ ] **LAYOUT-03** At ×1.25 rows grew with their text; none kept a fixed height.
- [ ] **A11Y-17** At 200% zoom in a 320px window the content reflows; nothing needs scrolling both ways.

## 4. Move around

Choose every sidebar and list row, open what they open and come back; open each control that offers choices; scroll
each root list.
- [ ] **NAV-04** Choosing a row shows its content in the next column of the same window, never a new window or a sheet.
- [ ] **NAV-08** A root list's large title turns into the inline title as it scrolls.
- [ ] **LIST-05** In a grouped list a row that pushes a view has a chevron; in a sidebar, or a column whose choice shows beside it, none.
- [ ] **LIST-12** Choosing in a navigation list keeps the row highlighted; choosing an option moves the checkmark.
- [ ] **LIST-08** An info button shows more about its row; it never navigates.
- [ ] **LIST-13** Several rows are selected only after `Select`, with the actions in the bar; leaving the mode clears the selection.
- [ ] **LIST-15** In a trailing swipe, `Delete` is red and outermost.
- [ ] **CTRL-09** A switch takes effect when flipped, unless its sheet is committed with Done.
- [ ] **CTRL-13** A segment changes the view; none runs a command.
- [ ] **CTRL-17** A menu of values updates its button (a pop-up); a menu of commands keeps its label (a pull-down).
- [ ] **CTRL-18** A pull-down holds three items or more, and the view's main action is not only inside it.
- [ ] **CTRL-19** A slider's minimum is at its leading end.
- [ ] **CTRL-21** A disclosure row opens what it names, right under it.
- [ ] **MENU-03** If one item of a kind has a context menu, every item of that kind has it, wherever it appears.
- [ ] **MENU-04** Every context-menu action is also somewhere in the main interface.
- [ ] **MENU-15** Choosing a menu item closes the menu and acts; an undoable one asks nothing.
- [ ] **INPUT-20** An item worth a window of its own has `Open in New Window` in its context menu.
- [ ] **HELP-02** Closing and reopening the window brings back the selection, the column and the scroll position.

## 5. Each form, with wrong values

For each form: leave it empty; type a wrong value and leave the field; fix it; send one the backend refuses (a name
that is taken); then Cancel with something typed.
- [ ] **FORM-16** An empty required field only disables the commit button; nothing turns red.
- [ ] **FORM-15** The commit button stays disabled until there is something to send.
- [ ] **FORM-22** A field speaks once left, then follows each keystroke; a new user name or password says what it lacks while it is typed.
- [ ] **FORM-20** The footer gives one problem at a time, the first from the top.
- [ ] **FORM-27** The problem goes on the keystroke that fixes it.
- [ ] **FORM-24** The backend's refusal lands in the same footer and goes when the input changes.
- [ ] **FORM-26** No alert ever reports input.
- [ ] **FORM-13** A search field, or one whose text is replaced rather than edited, clears with its trailing button.
- [ ] **CTRL-06** A commit that takes more than a second disables itself and shows a spinner (throttle the network).
- [ ] **WRITE-15** `Done` keeps the work and closes; `Cancel` throws it away.
- [ ] **WRITE-17** A label ends in `…` exactly when it asks for more input before acting.
- [ ] **MODAL-17** Cancel, Escape or a click outside a sheet holding typed input asks before discarding it.
- [ ] **MODAL-20** A popover closed by a click outside keeps what was changed in it.

## 6. Each delete, and each alert

source.md's first step listed every alert and every destructive action. Do each once.
- [ ] **MODAL-08** The alert is for a problem that needs an answer or an uncommon act that cannot be undone; a recoverable delete runs at once.
- [ ] **FEED-19** A delete that can be undone runs at once and offers Undo where the person is looking, and ⌘Z.
- [ ] **INPUT-22** ⌘Z and ⇧⌘Z undo and redo each edit and delete, many steps back.
- [ ] **INPUT-23** After an undo the restored item is scrolled into view and selected.
- [ ] **WRITE-16** `Delete` is for what stops existing; `Remove` takes an item out of a list or folder.
- [ ] **MODAL-23** A full-window cover closes only with the `Done` the person chooses.

## 7. Slow, empty and failing data

Throttle the network, start with no data, then make the backend fail (or open the stories made for those states).
- [ ] **FEED-13** While loading, a spinner or placeholders; `No Items` only once the answer came back empty.
- [ ] **FEED-11** One part loading never blocks the rest of the window.
- [ ] **FEED-03** Work of known size shows a meter and the amount; a spinner only when the size is unknown.
- [ ] **FEED-05** Progress moves evenly; a stall says so.
- [ ] **FEED-06** A spinner never turns into a bar mid-task.
- [ ] **FEED-08** Long work has a `Cancel`.
- [ ] **FEED-12** Data that changes updates by itself; Reload is an extra.
- [ ] **FEED-16** The failure banner's fix (`Retry`) works, and the banner goes when it succeeds.
- [ ] **FEED-18** A toast only confirms, and nothing needed is only in it.
- [ ] **A11Y-20** A toast or banner waits while pointed at or focused and can be dismissed; nothing needed leaves on a timer.
- [ ] **FEED-21** A notification only while the app is not in front, and never for an error.
- [ ] **INPUT-21** Hidden or minimised, media pauses and the person's own work carries on.
- [ ] **HELP-03** On a fresh profile the app works at once; setup waits until a feature needs it.
- [ ] **HELP-04** Access is asked for when the feature is first used, with one sentence on why.
- [ ] **HELP-06** Onboarding can be skipped, shows once, and can be found again.
- [ ] **HELP-09** A tip shows once, near its feature, to someone who has not used it, and can be dismissed.

## 8. Drag and drop, and motion

- [ ] **INPUT-11** Items that can be moved can be dragged to the lists, columns and windows that take them.
- [ ] **INPUT-12** Only a place that accepts the item highlights while it is over it.
- [ ] **INPUT-13** A failed drop goes back and changes nothing.
- [ ] **INPUT-14** A drop inside the same container moves; into another one copies; ⌥ forces a copy.
- [ ] **INPUT-15** Dropped items are selected in their new place, and ⌘Z undoes the drop.
- [ ] **MOTION-10** A view leaves the way it came; a popover grows from its anchor; a sheet rises and falls.
- [ ] **MOTION-11** A click during a transition lands; nobody waits for an animation.
