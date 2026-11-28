# Menus

Three kinds of menu appear in vos: the context menu on an item (secondary click, long press, or the Menu key /
Shift+F10), the pull-down menu of a button (More, Add, Sort), and the pop-up menu of a picker (`ui-picker`, which
uses the browser's own). Text has the browser's edit menu (Cut, Copy, Paste). The shell's `ContextMenu` draws the
iPadOS menu: a material card, 44 pt rows of Body text with a trailing glyph, hairlines between items, red destructive
items, and the menu keyboard pattern.

## Order inside a menu

1. The most used actions for this item (Open, Reply, Rename…).
2. Related groups, each separated: arranging (Move, Duplicate, Pin), sharing (Share, Copy Link), information (Get
   Info).
3. Destructive actions last, red (Delete, Remove).

---

### MENU-01 Menus are the shell's menu
- **Rule:** A context or pull-down menu is the shell's `ContextMenu` (through the kit or the host when an app needs
  one); an app never draws its own floating list of actions.
- **Why (iPadOS):** Menus; Context menus.
- **Check:** `source`; `auto`: an app element with `role="menu"` outside `ContextMenu`, or a floating list of buttons
  shown on `contextmenu`.
- **Wrong / Right:** a `div.dropdown` of links / the shell's menu.

### MENU-02 A context menu opens three ways
- **Rule:** An item with a context menu opens it on a secondary click (or Ctrl+click), a long press, and from the
  keyboard (Menu key or Shift+F10), and its control says so (`aria-haspopup="menu"`).
- **Why (iPadOS):** Context menus.
- **Check:** `interact`; `auto`: an element with a `contextmenu` listener and no `aria-haspopup`.
- **Wrong / Right:** a menu reachable only by right-click / all three, like `AppIcon`.

### MENU-03 Context menus are everywhere or nowhere
- **Rule:** If one item of a kind has a context menu, every item of that kind does, across the app.
- **Why (iPadOS):** Context menus.
- **Check:** `interact`.
- **Wrong / Right:** containers have a menu in the list but not in search results / both have it.

### MENU-04 Nothing lives only in a context menu
- **Rule:** Every action in a context menu is also reachable in the main interface (a bar button, the detail view, a
  swipe action plus edit mode).
- **Why (iPadOS):** Context menus.
- **Check:** `interact`.
- **Wrong / Right:** `Duplicate` only on right-click / also in the detail view's More menu.

### MENU-05 Short, with few groups
- **Rule:** A context menu holds the handful of actions most likely in this context, in at most three groups; rare or
  advanced commands are not in it.
- **Why (iPadOS):** Context menus.
- **Check:** `auto`: a menu with more than 12 items or more than 3 separated groups (user-generated lists, such as
  recent files, excepted).
- **Wrong / Right:** 20 items covering every setting / 7 items in 3 groups.

### MENU-06 Frequent first, related together
- **Rule:** The most used items come first (nearest where the menu opened) and logically related commands stay in one
  group even if some are rarely used.
- **Why (iPadOS):** Menus.
- **Check:** `look`.
- **Wrong / Right:** `Get Info` first and `Open` sixth / `Open` first.

### MENU-07 Destructive items go last
- **Rule:** Delete, Remove and other destructive items are the last group of a context or pull-down menu (and red,
  COLOR-09).
- **Why (iPadOS):** Context menus.
- **Check:** `auto`: in a `[role=menu]`, an item named `/^(Delete|Remove|Erase|Discard|Move to Trash)\b/` followed by a
  non-destructive item.
- **Wrong / Right:** `Delete` between `Rename` and `Share` / `Delete` at the bottom.

### MENU-08 Item labels are verbs without articles
- **Rule:** A menu item is a short verb phrase in title case with no articles: `Rename`, `Move to Folder…`, `Copy
  Link`.
- **Why (iPadOS):** Menus.
- **Check:** `auto`: a `menuitem` whose name contains ` the `, ` a ` or ` an ` as a word.
- **Wrong / Right:** `Copy the Link` / `Copy Link`

### MENU-09 Glyphs for all items of a group, or none
- **Rule:** A menu item's glyph is a standard one (CTRL-27) shown at the trailing end; within a group, every item has a
  glyph or none does. Leave a glyph out rather than use an unclear one.
- **Why (iPadOS):** Menus.
- **Check:** `auto`: a menu group (between separators) in which some items have an icon and others do not.
- **Wrong / Right:** `Rename ✎`, `Duplicate`, `Share ⇪` / glyphs on all three.

### MENU-10 Context menus hide what does not apply
- **Rule:** A context menu shows only actions that apply to the item now; it never shows dimmed items. A pull-down or
  pop-up menu dims unavailable items instead of hiding them.
- **Why (iPadOS):** Context menus; Menus.
- **Check:** `auto`: a context-menu item with `disabled` or `aria-disabled="true"`.
- **Wrong / Right:** a dimmed `Stop` on a stopped container's context menu / no `Stop` there.

### MENU-11 No shortcuts in context menus
- **Rule:** A context menu does not show keyboard shortcuts; shortcuts are shown where the app lists its commands
  (INPUT-04).
- **Why (iPadOS):** Context menus.
- **Check:** `auto`: a context-menu item whose text contains `⌘`, `⌥`, `⌃` or `⇧`.
- **Wrong / Right:** `Rename ⌘R` in a context menu / `Rename`.

### MENU-12 One level of submenu
- **Rule:** A submenu is one level deep, holds about five items or fewer, and its label predicts it (`Sort By`); it
  replaces repeated words, never indentation.
- **Why (iPadOS):** Menus.
- **Check:** `auto`: a menu item that opens a submenu inside a submenu.
- **Wrong / Right:** `Sort by Name`, `Sort by Date`, `Sort by Size` / `Sort By ›` with `Name`, `Date`, `Size`.

### MENU-13 A checkmark shows what is in effect
- **Rule:** A menu item for an attribute that is on shows a checkmark (or a label that says the state, `Show Hidden` /
  `Hide Hidden`), never both.
- **Why (iPadOS):** Menus.
- **Check:** `look`.
- **Wrong / Right:** `Show Hidden ✓` / `Hidden Files ✓`, or `Show Hidden` that becomes `Hide Hidden`.

### MENU-14 Text keeps its edit menu
- **Rule:** Editable fields and selectable text keep the browser's edit menu (Cut, Copy, Paste); an item has either a
  context menu or an edit menu, never both, and an app does not add buttons that duplicate Copy and Paste.
- **Why (iPadOS):** Edit menus; Context menus.
- **Check:** `auto`: a `contextmenu` event on an `input`, `textarea`, `[contenteditable]` or a text selection whose
  default is prevented.
- **Wrong / Right:** right-click in a field opens the app's row menu / the field's own edit menu.

### MENU-15 Choosing acts at once
- **Rule:** Choosing a menu item closes the menu and acts immediately; an undoable action is not confirmed, and a
  destructive one that cannot be undone asks in an action sheet or alert.
- **Why (iPadOS):** Pull-down buttons; Edit menus.
- **Check:** `interact`.
- **Wrong / Right:** `Duplicate` asks "Are you sure?" / it duplicates; `Delete Volume` (not undoable) asks.
