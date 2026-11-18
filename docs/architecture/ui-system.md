# AgentOS server-driven UI: component kit, view protocol, bindings, Luau API

Status: the design the platform is built on, and still its reference for the view protocol, bindings, state and the
Luau authoring API. It is implemented: the host in `crates/` (`agentos-domain` holds the schema and the protocol,
`agentos-luau` the app API), the renderer in `client/src/renderer/src/view` and the kit in `packages/ui`. Where the
code and this text differ, the code wins: the generated `types/generated/agentos.d.luau` and
`docs/reference/components.md` are the exact catalogue. Sections 6 to 8 record the original type-pipeline proposal,
the prototype and the build plan; each opens with what was actually built. Sources of truth beside it:
`docs/architecture/ARCHITECTURE.md`, `docs/TESTING.md`, `docs/design/DESIGN_LANGUAGE.md`, `apps/README.md`.

Keywords MUST/SHOULD/MAY are used in the RFC 2119 sense. "Host" = the Rust process, "app" = one Luau VM,
"client" = the Vite + Svelte PWA, "view" = one window's UI instance.

---

## 0. Decisions at a glance

| Question | Decision | Why (short) |
|---|---|---|
| Who owns what | Host owns truth (SQLite, Luau state). Client owns **interaction** (drafts, selection, navigation, scroll, focus, sheets, sort). | Typing, scrolling and navigation must never wait for a round trip; data must never be wrong on the client. |
| Wire format | JSON-RPC 2.0 over the existing WebSocket. Three message classes: `view.tree` (full), `view.ops` (**keyed structural ops**), `data.patch` (**per-binding value updates**). | Structure changes rarely; data changes constantly. Keep them on separate, individually cheap channels. |
| Patch strategy | **Keyed reconciliation** (React/Flutter style: host diffs, emits `set/insert/remove/move/replace` addressed by key path) for structure. **Not JSON Patch.** Values never travel as tree edits: they flow through bindings. | Stable keys survive reorders and keep client-side state (scroll, focus, drafts) attached to the right node; ops are idempotent; JSON Patch addresses by array index and breaks under reorder. |
| Binding model | **Refs + a closed expression language** in the tree (`{"$ref":"client.search"}`, `{"$fn":"fmt.date",...}`); **sources** (live SQLite queries) declared separately; list rows are a **template sent once** plus **row data**. Two-way for props typed `Binding`. | The tree stays small and static; values are resolved on the client reactively (Svelte runes); no app code or expression VM on the client. |
| State tiers | ephemeral UI (implicit, client) / **client-managed** (declared, client authoritative) / **app state** (Luau, host authoritative) / **data** (SQLite, host authoritative, live). | Matches SwiftUI `@State` / `@Binding` / `@Observable` and LiveView + JS commands, with the line drawn explicitly. |
| Conflicts | Dirty field **shields itself** from remote changes; write-back carries `baseRev`; host **3-way merges text on the base it last sent**, scalars are human-wins; the losing side is kept as a revision. | An agent appending to a note while the user types must lose nothing and never move the caret. |
| Luau API | **Table-call constructors** (`ui.VStack { spacing = 2, child, child }`) that return plain node tables; handlers are Luau functions (never serialised: referenced by key path); **both** layers exist: constructors are sugar over the plain-table wire form, raw tables are accepted and validated identically. No chained modifiers. | Native Luau idiom (Roact/Fusion), fully typeable in `--!strict`, JSON-shaped, diffable. |
| Styling | Apps can **not** style. Only semantic props from closed token sets (spacing units, text styles, tints, surfaces, radii, sizes in pt). No CSS, raw colours, fonts, shadows. Data colours (labels, canvas fills) are *content*, auto-contrasted. | Design language, WCAG AA, dark mode, Reduce Transparency and re-theming all stay host-controlled and guaranteed. |
| Type pipeline | One **component schema** (YAML with JSON Schema fragments + `x-agentos` metadata) -> Rust validator tables, TS types + renderer contract tests, Luau `.d.luau` + constructors, docs + stories. | The catalogue has metadata (events, slots, a11y rules, `since`, tier) that plain Rust or TS types do not express; one file, four outputs, drift is a build failure. |

---

### 0.1 Architecture

```
 Luau VM (one per app)                Rust host                              Client (Vite + Svelte 5 PWA)
 ┌─────────────────────┐     ┌──────────────────────────────┐        ┌───────────────────────────────┐
 │ render(w) -> tree   │────▶│ ViewInstance (per window)    │  tree  │ ViewCore (per window)         │
 │ handlers (closures) │◀────│  tree + rev, handler table   │───────▶│  store: tree, state, client,  │
 │ app.state / w.state │     │  source subscriptions        │  ops   │         sources (all $state)  │
 │ db:query / db:exec  │     │  last-sent values (merge     │ patch  │ adapters: wire props -> kit   │
 │ app.tool{...}       │     │   bases), op log             │◀───────│ drafts, debounce, client      │
 └─────────┬───────────┘     │ Live queries: sqlite hooks   │ events │  state, scroll/focus memory   │
           │                 │ Agents / MCP tools ──────────┼──┐     └───────────────────────────────┘
           ▼                 └──────────────┬───────────────┘  │
   per-app SQLite  ◀─────────────────────────┘  tool writes ────┘ (same db path => same live updates)
```

Rules that fall out of the decided architecture:

1. **The client runs no app code and no app expressions.** It runs a fixed, host-versioned interpreter for
   tree + refs + the closed function set (section 3.2). Everything else is the host's.
2. **A tool call and a click are the same thing to the UI**: both write app state or SQLite; the UI follows
   through bindings. There is no "refresh" code path for agent changes (section 3.3).
3. **One view instance per window**, owned by the host, addressed by `viewId`. The same app can have many.
4. **The client is replaceable**: all it needs is the catalogue schema and this protocol. The prototype's
   `ViewCore` (framework-free) is that contract in code; Svelte adapters are the only framework-bound part.

Repo placement (as built): `packages/ui` is the kit (atomic layers, `testid` required). The renderer lives in the client,
`client/src/renderer/src/view`: `core/` is this protocol's client half and `atoms/ molecules/ organisms/ templates/ pages/`
are the adapters (wire props -> kit components). Adapters import only `@agentos/ui` and `svelte`.

---

## 1. Component catalogue

### 1.1 Conventions

- **Naming**: SwiftUI names where one exists (`NavigationStack`, `Picker`, `Sheet`, `ProgressView`), UIKit/Qt
  concept noted where it differs (`List` = `UITableView`/`QListView`, `Table` = `NSTableView`/`QTableView`).
- **Prop kinds**: `T` = value or expression (read-only, client-evaluated); `Binding<T>` = a ref the control
  also **writes** (two-way); `Source<R>` = a live row source; `Slot` = named child list; `Event(payload)`.
- **`on` handlers**: each event has a handler of `true` (send to app), or `Action[]` run on the client
  (and any `emit` action also goes to the app). Section 3.5.
- **Slots are always node lists** (`slots: { name: Node[] }`); a single-child slot takes the first.
- Every component accepts the **common props** below. Every interactive component additionally requires an
  accessible name (visible text or `label`), enforced by the validator (section 5.3).

**Common props (all nodes)**

| Prop | Type | Notes |
|---|---|---|
| `key` | string | unique among siblings in the same slot; **required for interactive nodes and children of dynamic containers**; auto `_n` otherwise |
| `testid` | string? | override of the derived id (section 5.4) |
| `label`, `hint` | T string | accessible name / description |
| `hidden` | T bool | removed from layout and a11y tree (`display:none`) |
| `disabled` | T bool | |
| `padding` | 0..8 or `{x,y,top,...}` | units of 4 pt |
| `frame` | `{minW,idealW,maxW,minH,idealH,maxH}` pt, or `"fill"` | layout, not styling; clamped to the window |
| `grow` | number | flex-grow |
| `align` | `start|center|end|stretch` | in the parent stack |
| `background` | surface token | section 5.1 |
| `foreground` | label token | |
| `tint` | palette token | re-points `--tint` for the subtree |
| `corner` | radius token | `none|xs|sm|md|lg|xl|full` |
| `help` | T string | tooltip |
| `shortcut` | string | `"cmd+n"`, shown in menus, active while the window is focused |
| `drag`, `drop` | see 1.8 | drag and drop |
| `contextMenu` | Slot | menu items for long-press / right-click |
| `onAppear`, `onDisappear` | Event | |
| `animation` | `default|none` | |

### 1.2 Layout (Tier 1)

| Component | Props | Events | Slots | Kit mapping |
|---|---|---|---|---|
| `Stack` (`VStack`/`HStack` are `axis` sugar) | `axis v|h`, `spacing 0..8`, `align`, `justify start|center|end|between|around`, `wrap` (h only: flow) | | children | **new** atom (today: ad-hoc CSS in each app) |
| `ZStack` | `align` (9 anchors) | | children (back to front) | new |
| `Grid` | `columns` (n or `[{fixed}|{flex}|{min,max}]`), `rows`, `spacing`, `rowSpacing`, `colSpacing`, `aspect` (cell w/h); child props `span`, `row`, `col` | | children | new |
| `ScrollView` | `axis v|h|both`, `indicators`, `scrollTo` (Binding<key?>), `offset` (Binding out, throttled), `snap`, `refresh` | `onScroll{x,y}` (throttled), `onRefresh` | children | new (`.scroll` utility today) |
| `Spacer` | `min` | | | new |
| `Divider` | | | | new (separator token) |
| `Group` | transparent container, no layout (holds presentation nodes, passes through) | | children | new |
| `Adaptive` | `breakpoint` (container px, default 560) | | `compact`, `regular` | new; chosen **on the client** by container width, so resizing a window never round-trips |
| `Scope` | `scheme light|dark`, `tint`, `background` | | children | new (instrument apps, section 5.1) |
| `Show` | `when` T bool | | children (then), `otherwise` | new, client-evaluated |
| `ForEach` | `source` Source<R>, `key` (field, default the source key), `row` template | | `row` | new, see 3.3 |
| `Card` | `size small|medium|large|auto`, `surface solid|material` | | children | `Card` atom |

### 1.3 Navigation (Tier 1 unless noted)

| Component | Props | Events | Slots | Kit mapping |
|---|---|---|---|---|
| `Screen` | `title`, `subtitle`, `titleDisplay large|inline|auto`, `backLabel`, `searchable {text: Binding, placeholder, scopes, selectedScope: Binding, suggestions: Source}`, `refresh`, `background plain|grouped` | `onSearchSubmit`, `onRefresh` | children (content), `toolbar.leading|trailing|principal|primaryAction|secondaryAction|bottom|keyboard` | `NavigationBar` + `ScreenTemplate` (`accessory` snippet hosts search) |
| `NavigationStack` | `path` Binding<Route[]> (client state), `rootKey` | `onPathChange{path}` | `root`, `dest:<route>` (templates with `route.*` in scope) | `NavigationBar` + `BackButton` + new push/pop transition |
| `NavigationLink` | `to` Route (`{name, ...params}`), `title` | | children (label) | `ListRow chevron` |
| `SplitView` | `columns 2|3`, `visibility` Binding `all|double|detail`, `sidebarWidth narrow|regular|wide`, `preferredCompact sidebar|content|detail`, `style balanced|prominentDetail` | | `sidebar`, `content`, `detail` | `SplitViewTemplate` (add third column; collapses to a stack below 700 px per DESIGN_LANGUAGE s5) |
| `TabView` | `selection` Binding, `style bottom|top|sidebar|page` | | children = tabs, each tab has slot `tabItem {label, icon, badge}` | `TabBar` + `TabBarItem` |
| `Toolbar` items | `ToolbarItem {placement}`, `ToolbarGroup`, `Menu`, `Button`, `Spacer` as slot content | | | `NavigationBar` leading/trailing |
| `Link` (T2) | `url`, `title` | | | new; opens through host with confirmation |
| `Breadcrumb` (T3) | `items` | `onSelect` | | new (Files) |
| `Inspector` (T3) | `presented` Binding, `width` | | children | new trailing pane (Coder, Reviewer) |

### 1.4 Collections

| Component | Props | Events | Slots | Kit mapping | Tier |
|---|---|---|---|---|---|
| `List` | `style plain|inset|insetGrouped|grouped|sidebar`, `source`/`row` (shorthand for one `ForEach` child), `selection` Binding (key or key[]), `selectionMode none|single|multi`, `editing` Binding, `rowHeight fixed|auto`, `separators`, `groupBy` (source field -> section headers), `hasMore` T bool, `anchor start|end` | `onTap{key}`, `onMove{key,beforeKey}`, `onDelete{key}`, `onRefresh` (client spinner until reply), `onLoadMore`, `onSelectionChange` | children (static rows and `ForEach`), `row`, `header`, `footer`, `empty`, `swipeLeading`, `swipeTrailing` (Buttons with `role`, `fullSwipe`) | `ListSection` + `ListRow` + `SidebarItem` | 1 |
| `Section` | `header`, `footer`, `collapsible`, `expanded` Binding | | children, `header`, `footer` | `ListSection` | 1 |
| `Row` | `title`, `subtitle`, `value`, `icon`, `iconTint`, `badge`, `chevron`, `tag` (selection value), `role normal|destructive|action` | `onTap` | `leading`, `trailing`, children (custom content) | `ListRow` (needs a children slot for custom rows; today title/subtitle strings + leading/trailing snippets) | 1 |
| `Form` | = `List style=insetGrouped` with control rows; sugar | | children | `ListSection` | 1 |
| `LabeledContent` | `label`, `value` | | children | `ListRow value` | 1 |
| `DisclosureGroup` | `expanded` Binding, `label` | | children, `label` | new | 1 |
| `Table` | `columns [{id, title, field, width, align, sortable, format}]`, `source`, `sort` Binding `{column,dir}` (is a source param: the host sorts), `selection`, `rowHeight` | `onSort`, `onTap{key}`, `onMove`, `onColumnResize` | `cell:<column>` optional templates | new | 2 |
| `LazyGrid` (collection) | `source`, `minItemWidth`, `spacing`, `selection`, `aspect` | `onTap`, `onMove` | `item` | new | 2 |
| `Outline` (tree) | `source` (rows with `parent`), `expanded` Binding<key[]>, `selection`, `lazyChildren` (`onExpand{key}` fetches) | `onExpand`, `onTap` | `item` | new (Files, Coder file tree) | 2 |
| `Skeleton` | `lines`, `shape` | | | new (placeholder rows while a window loads) | 2 |

### 1.5 Controls

| Component | Props | Events | Slots | Kit mapping | Tier |
|---|---|---|---|---|---|
| `Button` | `title`, `icon`, `style filled|tinted|gray|plain|bordered`, `shape rounded|capsule|circle`, `size small|medium|large|fill`, `role normal|primary|destructive|cancel`, `fill` palette token (auto-contrast text), `selected` (aria-pressed), `loading`, `shortcut` | `onTap`, `onLongPress` | children (custom label) | `Button`, `IconButton` (+ `shape circle`, `fill`) | 1 |
| `Toggle` | `on` Binding<bool>, `title`, `style switch|checkbox|button` | `onChange` | label | `Toggle` | 1 |
| `Slider` | `value` Binding<number>, `min`, `max`, `step`, `ticks`, `minLabel`, `maxLabel` | `onEditingChange{editing}` | | `Slider`, `IconSlider` | 1 |
| `Stepper` | `value` Binding, `min`, `max`, `step`, `title` | | | new | 1 |
| `Picker` | `selection` Binding<value|value[]>, `options` Source or `[{value,label,icon,disabled}]`, `style menu|segmented|wheel|inline|navigation`, `multiple` | `onChange` | `option:<value>` custom | `SegmentedControl` (segmented), `Menu` (menu), `ListSection` + checkmark (inline), new wheel | 1 (wheel: 2) |
| `DatePicker` | `value` Binding<date>, `kind date|time|dateTime`, `style compact|graphical|wheel`, `min`, `max`, `step` | `onChange` | | new | 1 |
| `ColorPicker` | `value` Binding<color>, `palette` (tokens) or `custom`, `opacity` | | | new (swatch grid; Settings accent, Tasks labels) | 1 |
| `TextField` | `text` Binding, `placeholder`, `kind text|email|url|number|tel|search`, `lines`, `submitLabel`, `autocapitalize`, `autocorrect`, `validate {required, pattern, min, max, maxLength}`, `sync` (see 3.4), `mask?` | `onSubmit`, `onFocusChange` | `prefix`, `suffix` | `TextInput`, `TextFieldRow` | 1 |
| `SecureField` | as `TextField` minus `kind`; reveal toggle | `onSubmit` | | `TextInput type=password` | 1 |
| `TextEditor` | `text` Binding, `placeholder`, `font body|mono`, `readOnly`, `sync`, `selection` Binding out `{from,to}`, `spellcheck` | `onFocusChange` | | new (`<textarea>`) | 1 |
| `SearchField` | `text` Binding, `placeholder`, `scopes`, `suggestions` Source | `onSubmit`, `onCancel` | | `SearchField` | 1 |
| `Shortcut` | `shortcut` (invisible keyboard command; shown in the window's command list) | `onTrigger` | | new | 1 |
| `Rating` | `value` Binding, `max` | | | new | 3 |
| `TokenField` | `tokens` Binding<string[]>, `suggestions` Source | | | new | 2 |

### 1.6 Presentation

Presentation nodes are **portal nodes**: they render nothing in flow and mount in the window's overlay layer;
they may be children of any container (typically in a `Group` at the root).

| Component | Props | Events | Slots | Kit mapping | Tier |
|---|---|---|---|---|---|
| `Sheet` | `presented` Binding<bool>, `style form|page|fullScreen`, `detents [small,medium,large]`, `dismissible`, `item` (Binding<value?> alternative to `presented`) | `onDismiss` | children | `Sheet` (add detents, item form) | 1 |
| `Popover` | `presented` Binding, `anchor` (key, default parent), `edge`, `arrow` | `onDismiss` | children | new; collapses to a sheet on compact width | 1 |
| `Alert` | `presented` Binding, `title`, `message`, `fields [{key,placeholder,secure}]` (prompt) | `onDismiss` | `actions` (Buttons: `role`, preferred) | `Alert` (actions become events) | 1 |
| `ConfirmationDialog` | as `Alert` without fields, `anchor` | `onDismiss` | `actions` | new (popover on regular, action sheet on compact) | 1 |
| `Menu` | `title`, `icon`, `primaryAction` | | items: `Button`, `Toggle`, `Divider`, `Picker style=inline`, nested `Menu`, `Section` | `Menu` (static items today: needs nested, toggles, pickers) | 1 |
| `ContextMenu` | the common `contextMenu` slot | | | `Menu` | 1 |
| `Toast` | **imperative**: `w:toast{text, icon, kind info|success|error, action {title, onTap}, duration}` -> `view.command` | | | new | 1 |
| `Banner` | `kind`, `text`, `action`, `dismissible` (inline) | `onDismiss` | | new | 2 |
| `Tooltip` | the `help` prop | | | native `title` first, custom later | 1 |

### 1.7 Display

| Component | Props | Slots | Kit mapping | Tier |
|---|---|---|---|---|
| `Text` | `text` (positional in Luau), `style largeTitle|title1..3|headline|body|callout|subheadline|footnote|caption1|caption2|display`, `weight`, `foreground`, `align leading|center|trailing`, `lines n|0`, `truncation tail|middle|head`, `selectable`, `digits tabular`, `fit` (shrink to box; `display` only), `heading 1..6`, `live off|polite|assertive` | | `Text` | 1 |
| `Label` | `title`, `icon`, `display titleAndIcon|titleOnly|iconOnly` | | new | 1 |
| `Icon` | `name` (kit icon set), `size`, `weight`, `tint` | | `Icon` | 1 |
| `Image` | `src {asset}|{url}|{file}`, `fit fit|fill`, `size`, `corner`, `alt` (required unless `decorative`), `placeholder` | | new | 1 |
| `Badge` | `value` (number|string, omit = dot), `tint`, `max` | | `Badge` | 1 |
| `ProgressView` | `value` 0..1 or null, `style linear|circular|spinner`, `label` | | `ProgressBar`, `ActivityIndicator` | 1 |
| `Gauge` | `value`, `min`, `max`, `style linear|circular|capacity`, `tint`, `label`, `currentValueLabel` | | new | 2 |
| `Markdown` | `text`, `extensions [tables, tasklist, code]`, `selectable` | | `RichText` (desktop app, subset) promoted + full GFM subset; **no raw HTML, no `{@html}`** | 1 |
| `Avatar` | `name`, `image`, `contact` (agent/contact id resolved by the host), `size xs..xl`, `status`, `tint` | | `AgentAvatar` (desktop app) promoted to kit | 1 |
| `Chip` | `text`, `tint`, `removable` | | new | 2 |
| `EmptyState` | `icon`, `title`, `message` | `action` | new (DESIGN_LANGUAGE s8 requires empty states) | 1 |
| `Chart`, `Sparkline` | `source`, `kind`, ... | | new | 3 |

### 1.8 Advanced (issue-driven)

| Component | For | Props | Events | Notes | Tier |
|---|---|---|---|---|---|
| **DnD** (`drag`, `drop` common props) | Tasks kanban (#10), reorder | `drag {type, payload, preview?}`; `drop {accepts[], highlight}` | `onDrop{type, payload, target{key, beforeKey, edge}}` | Pointer-event based (HTML5 DnD is not touch-friendly); drag ghost, auto-scroll and hit-testing are client-side; the drop is the only message. **Every draggable MUST have a non-drag path** (`contextMenu` "Move to...", keyboard `Alt+Arrow`) | 1 |
| `Canvas` | Diagrams (#18) | `viewport` Binding `{x,y,zoom}` (client state, synced on idle), `selection` Binding<id[]>, `tool` Binding `select|pan|draw|connect`, `grid`, `snap`, `shapes` Source<Shape>, `bounds` (out) | `onTap{id,point}`, `onDrag{ids,delta,phase}`, `onTransform{ids,matrix}`, `onDrawEnd{points}` | Shapes are **data rows** `{id,kind,x,y,w,h,text,fill,stroke,...}` (rect, ellipse, path, line, text, image, group) plus `Edge {from,to,router,markers}`; the host sends only shapes intersecting `bounds`; pan/zoom/drag are 100% client with a local transform overlay committed on `phase: end`. Fill/stroke use palette tokens or data colours | 3 |
| `CodeEditor` | Coder (#16) | `doc` Binding to a **`text` source**, `language`, `readOnly`, `lineNumbers`, `wrap`, `decorations` Source (line -> add|del|mod|error|warn|blame, message), `diagnostics` Source, `selection` Binding out | `onSave`, `onSelection`, `onComplete{pos}` (request/response) | CodeMirror 6 inside the adapter (lazy chunk); highlighting is client-side (Lezer); document sync uses `splice` ops (3.3) so a 5 MB file does not travel per keystroke | 3 |
| `DiffView` | Reviewer (#17), Coder git view | `diff` Source (files -> hunks -> lines), `comments` Source (anchored threads), `mode unified|split`, `whitespace`, `selection` | `onComment{file,line,side,text}`, `onReply{thread,text}`, `onResolve{thread,state keep|resolve|drop}`, `onExpand{file,hunk,dir}` (lazy context) | Same highlighter as `CodeEditor`; comment composer reuses `TextEditor`; threads reuse `Avatar` + `Markdown` | 3 |
| `ChatTranscript` | Agents (#7), Books chat (#14) | `thread` Source (**kind `thread`**, below), `anchor end`, `agents` (contact map), `typing`, `composer` on/off | `onSend{text,attachments}`, `onStop`, `onRetry{id}`, `onLoadOlder`, `onPermission{id,choice}`, `onOpenArtifact{ref}` | Rows are AG-UI messages `{id, role, parts[]}`; AG-UI events map 1:1 to source ops (3.3). Built from `MessageBubble`, `RichText`, `TurnActivity`, `PermissionCard`, `Composer` (all in the desktop app today) promoted to kit organisms | 2 |
| `CalendarGrid` | Calendar (#8), Reminders date | `mode month|week|day`, `date` Binding (anchor), `range` Binding **out** (visible start/end, used as a source param), `events` Source `{id,start,end,allDay,title,tint,owner}`, `weekStart`, `hours`, `selection`, `nowIndicator` | `onSelectDate`, `onCreate{start,end}` (drag to create), `onMove{id,start,end}`, `onResize{id,end}`, `onTap{id}` | New. Month grid doubles as `DatePicker graphical`. Times are `instant`/`date` typed (3.6) | 2 |
| `TerminalView` | Terminal (exists in desktop) | `stream` Source (bytes) | `onData`, `onResize` | xterm.js; only if Terminal becomes a Luau app | 3 |
| `Preview` | Files quick look (#15) | `file` | | host-rendered preview by mime | 3 |
| `WebView` | Browser (#12) | `tabs` ... | | **Open question** (section 8): iframe sandbox vs host-driven remote browser | ? |

### 1.9 What exists, what is new

Existing kit components the renderer maps onto: `Text`, `Button`, `IconButton`, `Icon`, `Badge`, `Toggle`, `Slider`,
`IconSlider`, `SegmentedControl`, `TextInput`, `TextFieldRow`, `SearchField`, `ProgressBar`, `ActivityIndicator`, `Card`,
`ListRow`, `ListSection`, `SidebarItem`, `NavigationBar`, `BackButton`, `TabBar`, `TabBarItem`, `Sheet`, `Alert`, `Menu`,
`SplitViewTemplate`, `ScreenTemplate`, `Scrim`, `Grabber`, `Material`. Promoted from the desktop app: `AgentAvatar`,
`RichText`, `MessageBubble`, `TurnActivity`, `PermissionCard`, `Composer`.

**New kit work** (each follows atomic layers, takes `testid`, has a story per state): layout atoms (`Stack`, `Grid`,
`ScrollView`, `Spacer`, `Divider`), `Stepper`, wheel `Picker`, `DatePicker`, `ColorPicker`, `TextEditor`, `Popover`,
`ConfirmationDialog`, `Toast`, `Gauge`, `Markdown`, `Avatar`, `Label`, `Image`, `EmptyState`, `DisclosureGroup`,
`Table`, `LazyGrid`, `Outline`, virtual list, DnD utility, navigation push/pop transition, 3-column split view, sheet
detents; later `Canvas`, `CodeEditor`, `DiffView`, `ChatTranscript`, `CalendarGrid`.

**Gaps in existing components that matter to the protocol**: `Alert.actions[].onclick` and `Menu.items[].onclick` are
closures (adapters build them from nodes: fine). `ListRow` takes strings, not content (needs a `children` snippet).
`Sheet` has no detents. `NavigationBar` has no search accessory wiring beyond a snippet. `SearchField.value` is
bindable but has no debounce (the adapter adds it).

**Renderer layering**: `Node.svelte` -> `registry[type]` -> adapter -> kit component. Adapters are the only place that
knows both the wire props and the kit props; an unknown `type` renders `Unsupported` (labelled, harmless).

### 1.10 Tiers

**Tier 1: Calculator, Notes, Tasks, Reminders, Contacts, Settings** (and the Agents shell, minus transcript).

- Layout: `Stack`, `ZStack`, `Grid`, `ScrollView`, `Spacer`, `Divider`, `Group`, `Adaptive`, `Scope`, `Show`, `ForEach`, `Card`
- Navigation: `Screen`, `NavigationStack`, `NavigationLink`, `SplitView` (2 and 3), `TabView`, toolbar slots, searchable
- Collections: `List` (all five styles, sections, swipe actions, reorder, multi-select + edit mode, pull to refresh,
  infinite load, windowing), `Section`, `Row`, `Form`, `LabeledContent`, `DisclosureGroup`
- Controls: `Shortcut`, `Button`, `Toggle`, `Slider`, `Stepper`, `Picker` (menu, segmented, inline), `DatePicker`, `ColorPicker`,
  `TextField`, `SecureField`, `TextEditor`, `SearchField`
- Presentation: `Sheet`, `Popover`, `Alert`, `ConfirmationDialog`, `Menu`, `ContextMenu`, `Toast`
- Display: `Text`, `Label`, `Icon`, `Image`, `Badge`, `ProgressView`, `Markdown`, `Avatar`, `EmptyState`
- Advanced: DnD (Tasks kanban), a11y-mandatory non-drag alternative

**Tier 2: Agents (#7), Calendar (#8), Books (#14), Files (#15)**: `Table`, `LazyGrid`, `Outline`, `ChatTranscript`
(+ `Composer`), `CalendarGrid`, `Gauge`, `Chip`, `TokenField`, `Banner`, `Skeleton`, wheel `Picker`, `Link`, `Preview`
basics, client-side `List.filter` for small static collections.

**Tier 3: Coder (#16), Reviewer (#17), Diagrams (#18), Browser (#12)**: `CodeEditor`, `DiffView`, `Canvas`,
`TerminalView`, `Rating`, `Chart`, `Breadcrumb`, `Inspector`, `WebView` (open).

Tier 3 components are heavy and each owns a third-party engine (CodeMirror, canvas hit-testing): they are built
and gated per app, not up front. Tier 1 and the protocol (sections 2 and 3) MUST be complete before the first app ships,
because they determine every later shape.


---

## 2. View protocol

Transport: the existing JSON-RPC 2.0 WebSocket (`/rpc`), text frames, `permessage-deflate`. New methods use
**by-name params objects** (the examples below). If the Rust peer keeps the SDK's positional convention, each
params object is wrapped as the single element of the array; nothing else changes. Method names are namespaced:
`session.*`, `view.*`, `state.*`, `row.*`, `source.*`, `client.*`, `data.*`, `theme.*`.

### 2.1 Node shape

```ts
interface Node {
  type: string                      // catalogue name: "Stack", "TextField", "List", ...
  key: string                       // unique among siblings of the same slot
  props?: Record<string, Expr>      // literal, {$ref}, or {$fn}; Binding props hold a {$ref}
  children?: Node[]                 // default slot
  slots?: Record<string, Node[]>    // named slots: "row", "toolbar.trailing", "sidebar", "dest:note"
  on?: Record<string, true | Action[]>   // events this node reports; true = send to the app
}
```

(`client/src/renderer/src/view/protocol.ts` is this, executable: it re-exports the types generated from the Rust schema.)

**Addressing.** A node's **path** is the keys from the window root, `/`-joined, with a `@slot` segment when the
node sits in a named slot: `main/list/@row/item/delete`. Paths are stable across renders because keys are.
Rules (validated by the host on every tree it emits, so a bad app fails in its own tests, not in the client):

- keys are unique per (parent, slot); auto keys `_0`, `_1`, ... are assigned to un-keyed nodes by the Luau
  constructors and are only legal on **non-interactive nodes outside dynamic containers**;
- a row template's root and its descendants are keyed once; instances are addressed `path + rowKey`;
- limits: depth <= 64, nodes <= 10 000 per view, message <= 1 MB, ops per batch <= 1 000.

**Events are declarations, not closures.** The tree says *which* events a node reports (`"on": {"tap": true}`);
the Luau closure stays on the host in the instance's handler table keyed `path#event`, and is replaced on every
render (so it always sees current state). Nothing executable ever crosses the wire.

### 2.2 Full tree, then keyed ops

`view.open` returns a full `tree` (plus state, client state, source declarations and the first data windows: one
round trip to a complete window). After that the host sends **ops** addressed by path:

| Op | Shape | Notes |
|---|---|---|
| `set` | `{op:"set", path, props:{name: Expr | null}}` | `null` removes the prop (back to the default) |
| `on` | `{op:"on", path, on:{event: Handler | null}}` | handler flags change |
| `insert` | `{op:"insert", parent, slot?, before: key|null, node}` | `before: null` appends |
| `remove` | `{op:"remove", path}` | idempotent |
| `move` | `{op:"move", path, before: key|null}` | reorder among siblings |
| `replace` | `{op:"replace", path, node}` | type change, same key |

`view.ops {viewId, base, rev, ops[]}`: `base` must equal the client's current `rev`. A gap (`base != rev`) or an op
that cannot be applied (`no node at path`) makes the client call `view.sync` for a **full snapshot** and replace its
tree, keeping drafts and local state. Resync is the only recovery path; the protocol never guesses.

**How the host produces ops.** `render(w)` (Luau) returns a tree; the host reconciles it against the instance's
previous tree React-style: same `(type,key)` -> recurse and diff props; sibling key sequences are diffed with an
LIS-based minimal-moves pass; new keys -> `insert`, missing keys -> `remove`, same key new type -> `replace`.
Render is triggered by *tracked state reads* (3.1) and coalesced to one render per view per frame; ops from one
render are one `view.ops` message.

**Why keyed ops, and why not the alternatives.**

| Strategy | Latency / size | Simplicity | Verdict |
|---|---|---|---|
| **Resend the full tree** | 1 to 30 KB per change; client re-diffs | Trivial | Kept as the **resync** path only. Wasteful for a keystroke in a 200-node window. |
| **JSON Patch (RFC 6902)** on the tree | Small | Addresses array **indexes**: an insert shifts every later op; `move` loses identity; client-side state (focus, scroll, drafts) cannot be attached to "child 3"; needs `test` ops for safety | Rejected for structure. Fine for data blobs (AG-UI `STATE_DELTA` stays opaque inside the `thread` source). |
| **Keyed ops (chosen)** | ~50 to 300 bytes for typical changes | Host needs a reconciler (a few hundred lines in Rust; the algorithm is textbook) | Idempotent, order-tolerant within a batch, identity-preserving: a node keeps its Svelte instance, scroll, focus and draft because its **path** is its identity. |
| **Per-binding value updates only** | Smallest | Cannot express structure | Chosen **for data** (2.3), together with keyed ops for structure. |
| HTML fragments (Hotwire/Turbo Streams) | Medium | Great for server-rendered web, but no typed components, no client state, no bindings, needs the client to trust markup | Rejected (also violates "no `{@html}`"). |
| Statics/dynamics split (LiveView) | Very small | Requires templates the client can hold | Our `Expr` refs are the same idea (the template is the tree, the dynamics are refs + source data) but generalised to typed components. |

The key observation: **values change thousands of times more often than structure.** Typing, agent appends, counters,
row updates, live query refreshes: all `data.patch`. Structure (a sheet appears, an empty state toggles, a tab is
added) is `view.ops`. Separating them means the common path never runs the reconciler and never touches the tree.

**Latency budget** (host on localhost, p95): event on the wire 0.5 ms; Luau handler 1 to 3 ms; render + diff for
<= 500 nodes 3 ms; coalescing up to one frame (8 ms average, only when changes are bursty; a lone change flushes
immediately); client apply <= 2 ms; paint. Target **event -> pixels <= 30 ms** on the same machine. Everything the
user does continuously (typing, scrolling, drag, sheet, navigation, selection) never waits on any of this (3.5).

**Ordering guarantee.** All ops and data patches caused by handling an event are delivered **before** that event's
response, so the response means "the UI already reflects it" and optimistic state can be settled on it.

### 2.3 Data patches

`data.patch {viewId, state?, sources?}`: values, never structure.

```jsonc
{
  "viewId": "v1",
  "state":   { "draft.title": { "v": "Groceries", "r": 14 } },          // per state path: value + revision
  "sources": {
    "s_notes": { "kind": "rows", "total": 214, "from": 0,
                 "keys": [41, 7, 12],                                  // the whole window, in order
                 "rows": { "41": { "id": 41, "title": "Plan", "updated_at": "2026-11-18T09:12:00Z" } },  // ONLY changed rows
                 "drop": [3] },                                        // rows that left the window
    "s_note":  { "kind": "row", "rev": 9, "row": { "id": 7, "body": "...", "rev": 9 } }
  }
}
```

State values replace by path (apps keep paths granular; big collections belong in sources). A `rows` source
patch carries the window's **ordered key list** plus **only the rows whose content hash changed**: inserts,
removals and moves are all "a different key list", so there is no per-row op vocabulary to get wrong and a patch is
idempotent. `text` sources use `splice` and `thread` sources use `append` (3.3).

### 2.4 Events (client -> host)

```jsonc
// request: "view.event"; the result {"ok":true} arrives after the handler finished
{ "viewId":"v1", "rev":42, "seq":17, "path":"main/list/@row/item/delete", "rowKey":7,
  "event":"tap", "payload":null }
```

- `path` + optional `rowKey` address the node instance; `event` is from the catalogue; `payload` is typed per
  (component, event) by the schema (`change {value}`, `move {key, beforeKey}`, `drop {...}`, `select {keys}`).
- `rev` is the tree revision the user saw; the host resolves the handler by `path#event` against the **latest**
  handler table; if the node no longer exists it answers `{ok:false, reason:"stale"}` and the client does nothing.
- `seq` is per-view monotonic: used to dedupe on reconnect and to key optimistic undo logs. Events are **not**
  auto-retried after a reconnect (a tap is not idempotent); drafts and state writes are (3.4).
- Handler errors come back as JSON-RPC errors (`-32010 AppError`, message safe to show) and the client shows them as a
  toast (dev: with the Luau stack).
- The handler receives `e = {key, row, value, payload, actor}`: `row` is the host's last-sent snapshot of that row (so
  re-read by key if you need the latest), `actor` is the human at this client.
- **Before sending, the client flushes every dirty draft and waits for its ack** (`ViewCore.emit` -> `settle()`): the
  app never sees "Save" before the text the user typed before pressing it. (In the prototype: `view.test.ts`,
  "an event flushes the draft first".)

### 2.5 Per-window view instances

```
 shell window "w7" (Notes)  --view.open {app:"notes", window:"main", windowId:"w7", size, params}-->  host
   host: ViewInstance v1 = { app VM, window kind "main", tree, rev, handlers, subscriptions,
                             last-sent values, client-state mirror (synced entries), op log (last 256 batches) }
```

- Instance identity is `viewId` (host-generated). `windowId` is the shell's id, kept so a reconnect maps back.
- Many instances per app VM. They share `app.state` and SQLite; each has its own `w.state`, client state, sources
  and tree. A write to shared state re-renders every instance that read it.
- `params` carry a deep link (`{noteId: 12}`): the window's `onOpen(w, params)` seeds client state before the first tree.
- A render that errors keeps the **last good tree** and sends `view.error {message}`; the window shows an inline error
  banner (dev: with Luau trace). A crashing or killed VM sends `view.closed {reason}` to all its instances.
- **Visibility**: `view.visibility {viewId, visible:false}` (minimised, other stage) pauses pushes; the host keeps the
  dirty set and sends a single coalesced catch-up (or a full `view.tree` if the op log has rolled) on show.
- **Resize**: `view.resize {viewId, size, sizeClass}` is a *notification*, sent debounced, only if the app reads
  `w.env.*` in render. Layout adaptation itself (`Adaptive`, `SplitView` collapse) is client-side.
- **Reconnect / resume**: `session.hello {resume:{sessionId, views:[{viewId, rev}]}}`, then per view `view.open
  {resume:{viewId, rev, client:{...}, drafts:[...]}}`. Host alive -> op log since `rev` or a snapshot; host restarted ->
  a **fresh instance seeded with the client's persisted values**. Details and survival matrix: 3.5.
- **Close**: `view.close {viewId}` flushes drafts first, then the instance (and its source subscriptions) is destroyed.
  Closing a window never closes an app: the VM is lifecycle-managed separately (idle unload, tools stay registered).

### 2.6 JSON-RPC methods and notifications

Client -> host **requests** (all params objects; `viewId` omitted where obvious):

| Method | Params | Result | Purpose |
|---|---|---|---|
| `session.hello` | `{proto, kit, locale, timeZone, features[], resume?}` | `{proto, kit, hostVersion, sessionId, resumed}` | handshake, version negotiation, formatting env |
| `view.open` | `{app, window, windowId, size, params?, resume?}` | `ViewSnapshot` | create/resume a view: tree, state, client state, decls, first data |
| `view.sync` | `{viewId}` | `ViewSnapshot` | full resync |
| `view.event` | `{viewId, rev, seq, path, rowKey?, event, payload?}` | `{ok}` or error | user event |
| `state.set` | `{viewId, path, value, baseRev}` | `{rev, value, conflict?}` | two-way write to a server state path |
| `row.set` | `{viewId, source, key, field, value, baseRev}` | `{rev, value, conflict?}` | two-way write to a field of a bound row (host calls the source's `write`) |
| `source.set` | `{viewId, source, params?, range?{from,to}, anchor?}` | `SourcePatch` | change params and/or visible window |
| `view.close` | `{viewId}` | `null` | |

Client -> host **notifications**: `client.state {viewId, set:{name:value}}` (mirror of synced/persisted client
entries; FIFO ahead of any later event), `view.resize`, `view.visibility`, `session.bye`.

Host -> client **notifications**:

| Method | Params | Purpose |
|---|---|---|
| `view.tree` | `{viewId, rev, tree}` | full replace (after catch-up or large restructure) |
| `view.ops` | `{viewId, base, rev, ops[]}` | keyed structural ops |
| `data.patch` | `{viewId, state?, sources?}` | per-binding values and row windows |
| `client.set` | `{viewId, path, value, force?}` | the app drives client-managed state (open this note, push this route) |
| `view.command` | `{viewId, cmd, args}` | imperative one-shots: `toast`, `focus {key}`, `scrollTo {key|row}`, `announce {text}`, `copy {text}`, `openUrl {url}`, `haptic`, `closeWindow` |
| `view.error` | `{viewId, message, dev?}` | render or validation failure; last good tree stays |
| `view.closed` | `{viewId, reason}` | instance gone (app crash, uninstall) |
| `theme.set` | `{scheme, tint, textScale, reduceMotion, reduceTransparency, increaseContrast}` | host theme (section 5.2); session-wide, not per view |

### 2.7 Versioning

Two independent numbers, both announced in `session.hello`:

- **`proto`** (integer): envelope, method set and the semantics of ops/patches. Breaking changes bump it; host and
  client negotiate `min(proto)` and the host refuses sessions it cannot serve with a "reload the app" message. The PWA
  and host ship from the same binary, so skew only exists with a stale service-worker cache; the handshake turns that into
  a prompt, not a broken UI.
- **`kit`** (`major.minor`): the component catalogue schema. **Within a major, changes are additive only**: new
  components, new optional props, new enum values (clients ignore unknown props, and unknown enum values fall back to the
  default). A component's meaning is never changed in place: semantic changes get a new prop or a new component name.
  Each schema entry records `since` and `deprecated`; deprecations live for two minors.

When the client does not know a `type` (kit older than the app expects) it renders `Unsupported` (labelled, a11y
`role=note`). Nodes MAY carry a `fallback` slot; the generated Luau constructors for components added after kit 1.0 fill
it automatically (`Gauge` -> `ProgressView`), so a new app degrades on an old client instead of showing holes.
Apps declare `kit = ">=1.2"` in `app.manifest`; the host checks it at install against the running client kit.

### 2.8 Worked traces

**A. Calculator: press `7`** (shared `app.state` read in render, 3.1)

```
c->h  view.event {path:"main/_0/_1/keys/key-7", event:"tap"}
h     handler: s.entry = "7"; s.fresh = false   -> render -> diff (display text, AC->C label)
h->c  view.ops {base:3, rev:4, ops:[
        {op:"set", path:"main/_0/_1/display", props:{text:"7"}},
        {op:"set", path:"main/_0/_1/keys/key-clear", props:{title:"C"}} ]}
h->c  response {ok:true}
```

**B. Notes: an agent calls `notes.append` while note 7 is open and the user is typing in it** (full code in 4.4)

```
agent -> host  MCP tools/call notes.append {id:7, text:"- buy oat milk"}
h              Luau run(): db:exec("update notes set body = body || ... rev = rev + 1 ... where id = 7")
h              sqlite commit hook: tables {notes} dirty -> live queries on `notes`: s_notes (list), s_note (row)
h->c  data.patch {sources:{
        s_notes:{kind:"rows", total:214, keys:[7,41,...], rows:{"7":{id:7,title:"Groceries",preview:"milk ...",updated_at:...}}},
        s_note: {kind:"row", rev:10, row:{id:7, body:"milk\n- buy oat milk", rev:10, author:"Claude"}} }}
c   client TextEditor "body" is DIRTY (user typing): remote value held, not applied; the Markdown preview and the list row
    update immediately (they read the draft overlay / the source respectively)
c->h  row.set {source:"s_note", key:7, field:"body", value:"Milk", baseRev:9}   // debounce 400 ms; user capitalised "milk"
h     rev 9 != 10: merge3(base="milk" (what the host sent at rev 9), theirs="milk\n- buy oat milk", mine="Milk")
        -> hunks do not overlap -> "Milk\n- buy oat milk"; write() -> rev 11
h->c  response {rev:11, value:"Milk\n- buy oat milk", conflict:false}
c     adopts the merged value, caret mapped through the diff, scroll untouched
```

No Luau view code ran for the agent's change: the bindings did the work.


---

## 3. Bindings and state

This is the core of the design. Short version: **the tree is a template; values live in four tiers and reach the
tree through refs.** Everything the user does continuously is resolved on the client; everything that is a fact is
resolved on the host; the two meet at well-defined seams (sources, two-way bindings, client-state mirrors).

### 3.1 The four tiers of state

| Tier | Example | Authority | Lives in | Written by | Reaches the other side by | Latency to the user |
|---|---|---|---|---|---|---|
| **Ephemeral UI** | scroll offset, caret/selection range, focus, hover/pressed, IME composition, row swipe-open, text typed but not yet committed | client | the adapter / `UiMemory` keyed by node path | the user | never (unless a control declares `onScroll`, `selection` out-binding) | 0 |
| **Client-managed** (declared) | open sheet/alert/popover, navigation stack, selected tab, sidebar selection, list selection, search text, sort order, expanded sections, calendar anchor date | **client** | `store.client` (`$state`) | the user, client actions, or the app via `client.set` | mirrored to the host when declared `sync`/`persist` (3.5); read by sources as **params** | 0 |
| **App state** | Calculator accumulator, a form's validated draft, "is syncing" flag | **host** | Luau tables: `app.state` (shared by all windows), `w.state` (per window) | handlers, tools | `data.patch.state` for refs; **tracked reads** in `render` for structure | 1 RTT |
| **Data** | notes, tasks, contacts, messages | **host** | per-app SQLite | handlers, **agent tools**, anything | live **sources** (3.3) | 1 RTT + query |

How the Luau side distinguishes them (4.2): `w.client.x` is a **Ref** (the host has no authoritative value),
`w.state.x` is a **value** (read in render = tracked dependency), `db:query{...}` is a **Source**.

The rule of thumb for app authors: *if the user is mid-gesture or the value is only about how the screen is
presented, it is client-managed; if another window, an agent, or tomorrow's launch could care, it is app state or data.*

### 3.2 Refs and the closed expression language

A prop value is a **literal**, a **Ref**, or a **Fn** call:

```jsonc
{ "$ref": "client.search" }                                   // client-managed state
{ "$ref": "state.display" }                                   // app/window state (server)
{ "$ref": "src.s_note.body" }                                 // a field of a single-row source
{ "$ref": "src.s_notes.total" }                               // a source's total
{ "$ref": "row.title" }                                       // inside a row template: the current row
{ "$ref": "route.id" }                                        // inside a NavigationStack destination
{ "$ref": "env.sizeClass" }                                   // env: sizeClass, heightClass, locale, timeZone, scheme, now
{ "$fn": "concat", "args": [ {"$ref":"src.s_notes.total"}, " notes" ] }
```

A literal object that happens to contain `$ref` or `$fn` as its own key is written `{"$lit": ...}`.

**The function set is closed, pure and presentation-only**: `not and or eq ne lt gt add sub if coalesce concat len isEmpty
upper lower`, and `fmt.number fmt.currency fmt.percent fmt.bytes fmt.date fmt.time fmt.dateTime fmt.relative
fmt.plural fmt.list`. No loops, no recursion, no user functions, no I/O: it cannot hang or exfiltrate, it has a total
cost bound (nodes in the expression), and it is the same on every client. Anything beyond it is computed on the host
(3.6). (Implemented in `client/src/renderer/src/view/core/expr.ts`; an unknown function evaluates to `undefined`, never throws.)

Refs resolve **reactively** on the client: adapters call `view.evaluate(expr, scope)` inside `$derived`, which reads
Svelte `$state` proxies, so Svelte tracks exactly which store entries a prop depends on and re-renders only that
prop of that node. This is the same mechanism QML property bindings and SwiftUI's observation use, and it is why a
`data.patch` for `state.display` repaints one text node and nothing else.

**Resolution order** for `state.*` and `src.<id>.<field>`: **draft overlay first** (the value the user is currently
editing, 3.4), then the server value. That is why a `Markdown` preview bound to the same ref as a `TextEditor` updates
on every keystroke with no round trip.

**Binding props** (`Binding<T>` in the catalogue) accept a Ref and also **write** it. The write target is decided by
the ref namespace:

| Ref | Write path | Latency |
|---|---|---|
| `client.*` | `store.client[x] = v` immediately; source params observing it re-query; mirrored if `sync`/`persist` | 0 |
| `state.*` | draft overlay + debounced `state.set` | 0 (draft), ack later |
| `src.<id>.<field>` or `row.<field>` | draft overlay + `row.set` -> the source's `write(key, field, value)` | 0, ack later |
| anything else | not writable: the validator rejects a Ref in a `Binding` prop that points at a read-only namespace | |

### 3.3 Sources: data bound to live queries

A **source** is a host-owned, parameterised, live read of data. The tree references it by opaque id; the SQL and
the database never leave the host (the client cannot send SQL, only **param values**, bound as parameters and
type-coerced against the declared types).

```jsonc
// ViewSnapshot.sources
"s_notes": { "kind":"rows", "key":"id", "page":60, "debounce":250,
             "params": { "folder": {"$ref":"client.folder"}, "q": {"$ref":"client.search"} } },
"s_note":  { "kind":"row",  "key":"id", "writable":["body"],
             "params": { "id": {"$ref":"client.noteId"} } }
```

| Kind | Shape on the client | Typical use | Patch |
|---|---|---|---|
| `rows` | `{total, keys[], rows{key:row}}` (a window) | `List`, `Table`, `LazyGrid`, `Picker options`, `CalendarGrid events` | `keys` (window order) + changed `rows` + `drop` |
| `row` | `{row|null, rev}` | detail pane, form | whole row |
| `value` | scalar | counts, badges (`db:scalar`) | value |
| `text` | `{rev, length, value?}` | `CodeEditor` documents (large, edited) | `{op:"splice", at, del, ins, rev}`; client sends `text.splice` with `baseRev` |
| `thread` | rows `{id, role, parts[]}` | `ChatTranscript` | `rows` upserts **plus** `append: [{id, path:"parts.0.text", text}]` for streaming |

**Params are refs.** When `client.search` changes, the client (not the app) notices that source `s_notes` depends on
it, waits `debounce` ms and sends `source.set {source, params}`; the host re-runs the query and answers with the
first window. Stale replies are dropped by a per-source sequence number. (Prototype: `ViewCore.syncSources`,
`view.test.ts` "a client-state write is local; only the source params change reaches the host, debounced".)
Template-scoped sources (inside a `NavigationStack` destination) are instantiated per mounted instance with
`route.*` params and released on unmount.

**Live updates, including agent changes.**

1. At prepare time the host records the **tables each query reads** (SQLite authorizer callback: `SQLITE_READ` rows;
   catches joins, subselects, views).
2. Any write on the app's connection (a Luau handler, an `app.tool`, a migration, a background job) ends in a commit.
   The `commit_hook` hands the set of dirty tables (collected by `update_hook`) to the live-query scheduler.
3. Within one frame (16 ms, coalescing bursts, up to 50 ms under load) the scheduler re-runs, **in one read
   transaction** (consistent snapshot across list, detail and counts), every subscribed source of every open view
   whose dependency set intersects the dirty set.
4. Each result is diffed against what that view last received (per-row content hash, ordered key list) and a
   `data.patch` is sent only if something changed. **Cost is bounded by window size, not table size.**
5. Writes are single-writer (the host owns the connection), so there is no "someone else changed the file" case to detect.

An agent calling `notes.append` therefore needs no UI code and no `emit`: it writes SQLite, and every open window
that shows that note, its list row, the folder count, and the Markdown preview updates through its bindings
(trace in 2.8-B). `db:tx(fn)` batches several writes into one commit so views see one consistent change.

**Thread sources and AG-UI.** The Agents app binds `ChatTranscript` to a `thread` source maintained by the host's
AG-UI bridge. AG-UI events map to source patches 1:1:

| AG-UI event | Patch |
|---|---|
| `RUN_STARTED` / `RUN_FINISHED` / `RUN_ERROR` | upsert the thread row's `status`; `ChatTranscript` shows typing/stop and error rows |
| `TEXT_MESSAGE_START` | upsert `{id, role, parts:[{type:"text", text:""}]}` |
| `TEXT_MESSAGE_CONTENT` (delta) | `append {id, path:"parts.N.text", text}` (no re-send of the message) |
| `TEXT_MESSAGE_END` | mark part complete |
| `TOOL_CALL_START/ARGS/END/RESULT` | upsert/append a part `{type:"tool", name, args, status, result}` |
| `STATE_SNAPSHOT` / `STATE_DELTA` (RFC 6902) | applied host-side to the agent state; exposed as a `row` source if an app binds to it |
| reasoning / `CUSTOM` | parts `{type:"reasoning"}` / `{type:"custom"}` |

JSON Patch therefore still appears, exactly where it is the right tool (an opaque agent state blob), not in the view protocol.

### 3.4 Two-way bindings, drafts, optimistic updates and conflicts

Every editable control binds a ref and follows the same small state machine per bound path (implemented once, in
`ViewCore`, used by every adapter; prototype `view.ts` + `merge.ts`):

```
 clean ──edit──▶ dirty ──debounce/maxWait/blur/submit/event──▶ inflight ──ack──▶ clean
                  ▲  │                                            │
                  └──┴──edit while inflight: stays dirty, next write rebased on the ack
```

**Rules.**

1. **The user's value is shown immediately** (draft overlay in `store.state`/row cache). The control is *never* a
   server-controlled input: no caret jumps, no IME breakage.
2. **A dirty or inflight field shields itself.** A remote change for that path is *held* (`held = {v, r}`), not applied.
   This is LiveView's "never patch the focused input" rule, generalised to any dirty field, and bounded by `maxWait`.
3. **Write-back is debounced** (`sync="debounce:300"` default for text; `maxWait` 1 000 ms so continuous typing still
   lands; `blur` and `submit` modes for forms; `change` for toggles/sliders/pickers). It flushes immediately on blur,
   on window close/hide, before any `view.event` (2.4), and on reconnect. IME composition never flushes mid-composition.
4. **The write carries `baseRev`** (the revision the client's value was based on).
5. **The host decides, by binding type** (the schema marks each binding prop `text` | `scalar` | `collection`):
   - `baseRev == current` -> accept.
   - **text**: *three-way merge* of `base` (the value the host last **sent** this view at `baseRev`; the host keeps a
     ring of the last ~8 sent values per bound path, so it always has it), `theirs` (current), `mine`. Hunks that do
     not overlap compose; two pure insertions at the same point compose, theirs first; overlapping edits resolve to
     **mine** (the human at the keyboard wins) and `conflict:true` is returned, and the overwritten value is stored
     in the revision history. (Prototype: `merge.ts` with tests; production uses a real diff3, e.g. the `similar` crate.)
   - **scalar** (toggle, slider, stepper, picker, date, colour): **human wins** (a click is explicit intent); the
     overwritten value is recorded in the revision history. An app that needs strictness (approvals) declares
     `conflict = "reject"` on the source/state: the host answers `{conflict:true, value: current}`, the client adopts the
     server value, announces "changed elsewhere" (`aria-live`) and shows the field's `error` state.
   - **collection** (multi-select, tokens): set-union/difference of the user's *delta* against theirs when the
     control reports deltas; otherwise human wins.
6. **The ack carries the authoritative value.** If it differs from what was sent (a merge), the client adopts it and
   maps the caret and selection through the diff (common prefix/suffix), scroll untouched. If the user kept typing
   while the write was inflight, the draft stays and the next write is rebased on the ack.
7. **The originator does not get an echo**: the host sends other views `data.patch`, and this view the ack (an echo is
   harmless: ignored by `r <= baseRev`).
8. **Nothing is silently lost.** The losing side of a human-wins overlap is kept in a per-field revision ring
   (`_ui_revisions`), the client shows a toast "Your edit replaced an agent change. Undo", and agents can read the ring
   through the tool context. A non-overlapping merge needs no toast.

**Optimistic updates** beyond text:

- *Client actions* (3.5) are optimistic by construction: `ui.act.set(c.sheet, true)` runs on the client.
- *Event handlers that mostly succeed* may declare an optimistic client effect:
  `onTap = ui.act.seq{ ui.act.set(r.done, true), ui.act.emit("complete") }`. The client records the **before** value keyed by
  `seq`; the authoritative `data.patch` overwrites it; if the event's response is an error, the before value is restored
  (unless a newer authoritative patch has already replaced it) and a toast shows the error.
- *Drop/reorder*: the dragged item stays at the drop target in a "settling" state for up to 500 ms waiting for the new
  key order; if none arrives it animates back.
- *Row field toggles* (`Toggle { on = r.done }` in a Tasks/Reminders row): `row.set` with an overlay on that row field,
  same ack/revert semantics.

**Debounce and interaction with sources**: search text is a *client-managed* value (no draft/merge needed: nobody else
writes it); it only affects the host through the source params (debounce 150 to 250 ms).

### 3.5 Client-managed state, client actions, what survives what

**Declaration** (Luau, 4.2): `client = { name = {init, persist, sync} }`.

| Option | Values | Meaning |
|---|---|---|
| `init` | JSON | initial value (deep links may override via `params`) |
| `persist` | `none` (default) / `session` / `app` | `none`: in memory while the view is open. `session`: also in `sessionStorage`/IndexedDB keyed by `windowId`, so it survives reconnect, host restart and page reload but not closing the window. `app`: also stored on the **host** (`_ui_state(app, window, instance_key, name, json)`), restored into the next `view.open` of that window kind, so it survives closing the window and restarting the app |
| `sync` | `none` / `change` / `commit` | `change`/`commit`: mirrored to the host (`client.state`, FIFO ahead of any event) so handlers can `w.client:get(name)`. Implied (at least `commit`) by `persist` |
| `type` | schema | validated on restore and on `client.set` |

**Server-driven**: `w.client:set(name, v)` -> `client.set`. It applies immediately and overrides the user's value
(the app asked for it: "open this note", "push this route"), except on a **focused text control** with a different value
unless `force`.

**Client actions** are the declarative micro-language that runs *on the client* with no round trip. They exist so
presentation changes never wait for the app:

```ts
type Action =
  | { do:"set"; ref: string; value: Expr }      | { do:"toggle"; ref: string }
  | { do:"nav.push"; stack?: string; route: {name:string; [p:string]: Expr} }
  | { do:"nav.pop" } | { do:"nav.popToRoot" }
  | { do:"focus"; key: string } | { do:"scrollTo"; key: string }
  | { do:"copy"; text: Expr }
  | { do:"emit"; event?: string; payload?: Record<string, Expr> }   // to the app
  | { do:"seq"; actions: Action[] }
```

`present`/`dismiss` are `set` on the presentation's `presented` ref. A handler is `true` (just emit), an `Action[]`
(client first, `emit` entries to the app), or an app closure (emit). Prototype implements `set`, `toggle`, `emit`.

**What lives on the client, by default**: text being typed (draft), selection and caret, focus, scroll position,
open/closed state of sheets, popovers, alerts, menus, the navigation stack, selected tab, sidebar and list
selection, sort order, search text, expanded sections, swipe-open rows, drag state, the canvas viewport.

**Survival matrix**

| State | Reconnect (same host) | Host restart | Tab reload | Window close, reopen | App restart |
|---|---|---|---|---|---|
| Ephemeral UI (scroll, focus, caret) | kept (in memory; restored by node path) | kept | scroll restored (sessionStorage by `windowId`+path), focus not | gone | gone |
| Unacked drafts | **replayed** (`row.set`/`state.set` with `baseRev`, merge rules apply) | replayed, conflict rules apply | replayed from the IndexedDB outbox | flushed on close; if offline kept in outbox | outbox |
| Client state `persist=none` | kept | kept | gone | gone | gone |
| Client state `persist=session` | kept | kept | **kept** | gone | gone |
| Client state `persist=app` | kept | kept | kept | **kept** (host copy) | **kept** |
| `w.state` (per-window server state) | kept | gone unless the app persisted it | kept (instance lives) | gone | gone |
| `app.state` | kept | gone unless persisted via `app.kv`/db | kept | kept | gone |
| SQLite data | kept | kept | kept | kept | kept |
| Navigation stack | client state, `session` by default | kept | kept | gone (opt in to `app`) | gone |
| Open sheet / alert / popover | `none`: kept in memory only | kept | gone | gone | gone |

**Reconnect protocol.** The client keeps every open view mounted and dims it ("Reconnecting", read-only for server
actions, inputs still accept drafts). On a new socket: `session.hello {resume}` then `view.open {resume:{viewId, rev,
client, drafts}}`. Host alive (grace 60 s) -> `ViewSnapshot` or ops since `rev`; the client reconciles snapshot state
through the same `onState` rule as live patches (so dirty drafts are shielded, not clobbered) and replays drafts.
Host restarted -> a fresh instance that **adopts the client's persisted values** for declared entries, so selection,
route and mode survive a host restart without the app doing anything.

### 3.6 Computed values and formatting: client or server

| Concern | Where | Why |
|---|---|---|
| Dates, times, numbers, currency, percent, bytes, plurals, lists, relative time | **client** (`fmt.*` via `Intl`) | uses the *viewer's* locale and time zone (two devices see their own), no per-locale strings on the host, relative time ticks from a shared clock (`env.now`, minute resolution) with zero messages |
| Presentation logic: conditionals, concatenation, "n selected", button enabled from form validity, empty-state text | **client** `Expr` | no round trip, no flicker |
| Business/semantic computation: totals, permissions, search ranking, calculator arithmetic, anything an agent or notification must also read | **host** (Luau or SQL, incl. generated columns/views) | single source of truth, testable, agent-visible |
| Sorting, filtering, searching, grouping of data that does not fit in a window | **host** (source params) | only the host has the rows |
| Filtering a small static collection (settings panes, < ~500 rows already resident) | client `List.filter` (tier 2) | avoids a source for trivia |
| Strings given to agents, notifications, logs | **host** | the client is not involved |

Values travel typed, not formatted: `instant` (ISO-8601 UTC, `Z`), `date` (`YYYY-MM-DD`, floating, no zone: all-day
events, due dates), `time` (`HH:mm`, floating), numbers as JSON numbers, money as `{amount: integer minor units,
currency}`. The schema types fields so `DatePicker`, `CalendarGrid` and `fmt.date` treat `instant` and `date`
correctly (the classic reminders/calendar bug is formatting a floating date in the viewer's zone).

### 3.7 Lists of thousands

`List`/`Table`/`LazyGrid` virtualise: the client renders only visible rows plus overscan and asks the host for the
**window** it needs.

- **Window request**: `source.set {source, range:{from,to}, anchor?}` as the scroll position changes (debounced to
  animation frames, coalesced; the scrollbar thumb is sized from `total`). Window = visible + 1 page overscan each side,
  default `page: 60`. Rows outside +/-2 windows are evicted from an LRU row cache. While a window loads, `Skeleton` rows
  keep the geometry.
- **Host query**: the source's SQL is wrapped `select * from (<sql>) limit :n offset :from` and `select count(*) from
  (<sql>)` (`total: "exact"|"estimate"|"none"`; `estimate` skips the count for huge tables). Windowed sources MUST have a
  total order (`order by ..., <key>`); the Luau `db:query` rejects a windowed query without one.
- **Live updates re-run only the subscribed windows** and the count: 2 to 3 small queries, never the table. A row that
  moves out of the window is a `drop`.
- **Scroll anchoring**: the client anchors on the **key** of the first visible row, so inserts above the viewport
  (agent adds a note) do not shift what the user is reading.
- **Infinite load / pagination**: `loadMore` simply widens the window (`range.to += page`) when the viewport nears the end
  (`threshold` rows); `hasMore` comes from `total`. Chat-style lists use `anchor: "end"`: ranges count from the newest
  row, the view sticks to the bottom unless the user scrolled up, `loadOlder` prepends.
- **Selection** is a set of **keys** in client state, not indexes, so it survives windowing, sorting and live updates;
  "Select all" is represented as `{all:true, except:[...]}` so a 100 k selection is not enumerated.
- **Reorder** is `onMove {key, beforeKey}` (never indexes); apps store order as a fractional rank (`position REAL`) so a
  move is one row update.
- **Accessibility under virtualisation**: rows carry `aria-setsize` (= `total`) and `aria-posinset` (= absolute position),
  so a screen reader reports "7 of 4 312" and `Home`/`End`/`PageDown` request the target window directly.
- **Budgets**: a view subscribes to <= 16 sources, windows <= 500 rows, row payload <= 4 KB; the host rejects more.
  A 100 000-row table costs the client ~100 DOM rows and one 60-row query per scroll step.

### 3.8 Comparison and the choice

| System | Where state lives | How the UI updates | Two-way / input handling | What we take | What we leave |
|---|---|---|---|---|---|
| **SwiftUI** | `@State` (view-local), `@Binding` (reference), `@Observable` (model, access-tracked) | body re-evaluated for tracked reads, diff by identity (`id`) | `Binding` is a get/set pair | the value/Binding split (`w.state.x` vs `Ref`), access tracking for render deps, identity by key, `selection`/`presented` bindings | in-process closures; the client is not Swift |
| **Qt QML** | properties on objects | property bindings: expressions re-evaluated when dependencies change | `Binding {}` / two-way via signals | reactive refs resolved on the client; declarative `Behavior`-like animation tokens | arbitrary JS expressions (our `Expr` is closed so the wire stays data) |
| **Flutter** | `State` objects, `setState`, Provider/Riverpod | widget rebuild, keyed element reconciliation | controller objects (`TextEditingController`) | keyed reconciliation; controllers = our drafts | rebuild of whole subtrees on the client (our host rebuilds) |
| **Phoenix LiveView** | **server** (socket assigns) | server re-renders, sends minimal statics/dynamics diff | each event goes to the server unless `phx-debounce`; focused inputs are not patched; `JS` commands for client-only effects | server authority, diff over the wire, "don't patch the focused input", client-side commands (our Actions), debounce | HTML templates and DOM patching (we ship typed components) |
| **Hotwire (Turbo + Stimulus)** | server (HTML), Stimulus for sprinkles | HTML fragments, Turbo Streams targeted by DOM id | forms round-trip | targeted-by-id updates (our keyed ops) | untyped HTML, no client state model, markup trust |
| **Airbnb Ghost Platform** (public descriptions) | server-defined screens/sections, client component registry | screen/section replacement on action; typed shared schema and typed actions | actions are a typed contract | typed schema across platforms, component registry, typed actions, section as the unit of reuse | no live bindings (data changes = refetch), no client-managed state model |
| **Adaptive Cards** | none (card is a document) | card replaced on `refresh`/`Action.Execute` | `Input.*` collected on submit | host-config theming (our host-set tokens), strict schema + versioned fallback (`fallback`, `requires`) | no bindings or state; no live data |

**Choice: server-authoritative, LiveView-like transport, SwiftUI-like value model, with a split wire format.** The host owns
truth and structure (LiveView, Hotwire); the client owns interaction state declared in the tree (SwiftUI `@State`, QML
local properties) and resolves refs reactively (QML, SwiftUI observation); typed components and actions come from the
Airbnb/Adaptive Cards lineage; structure and data travel on different channels so the high-rate path is the cheap
one. What none of the precedents give us, and this design adds, is **live data bindings that include agent writes**
(sources) and an **explicit conflict contract for the case where two actors, one human and one agent, edit the same
text** (3.4).


---

## 4. The Luau authoring API

### 4.1 Style: table-call constructors over a plain-table wire form

Three options were weighed.

| Option | Example | Verdict |
|---|---|---|
| SwiftUI-style chained modifiers | `ui.Text("Hi"):font("title"):foreground("secondary")` | Reads well, but in Luau every modifier returns the generic node type, so per-component props cannot be type-checked or autocompleted; errors point at the chain; and it hides the data shape. Rejected. |
| Plain tables + reconciler | `{ type = "Text", props = { text = "Hi" } }` | Maximally simple and exactly the wire form, but noisy, no autocomplete, typos become runtime errors. Kept **as the escape hatch** (generated UI, JSON from a database). |
| **Table-call constructors (chosen)** | `ui.Text { "Hi", style = "title" }` | Luau-native (`f{...}` call sugar; Roblox's Roact and Fusion use the same idiom), JSON-shaped, fully typeable from generated `.d.luau` in `--!strict` with `luau-lsp`, children are just positional entries, slots are just named keys, diffs and snapshots are readable. |

So: **both layers exist**. Constructors are thin: they validate, auto-key, capture the caller's `file:line` (dev only, for
error messages) and return the **plain node table** of 2.1. `render` may return raw tables too; they pass through the same
validator. There are no modifiers: universal props (padding, frame, background, ...) are ordinary keys.

**Reading the examples**

- `ui.X { a, b, key = "k", prop = v, slotName = ..., onTap = fn }`: positional entries are children (or the text/title for
  `Text`, `Label`, `Button`, `Row`), named keys are props, slots, and handlers. Arrays and `nil` in child position are
  flattened/skipped; `ui.each(list, fn)` maps a Luau list to children.
- A **handler** is a Luau function (reported by key path, never serialised), a client **Action** (`ui.act.*`), or a
  list mixing both.
- `ui.Text { s.display }` reading **app state in render** registers a dependency: the host re-runs `render` when it
  changes and sends the minimal `set` op. `w.client.x` / `source.field` / `r.field` are **Refs** (values the host does not
  own): they resolve on the client and cost no re-render.
- `and`, `or`, `not` are Luau keywords, so the expression helpers are `ui.all`, `ui.any`, `ui.negate`.

### 4.2 API reference

```lua
local app = require("@agentos/app")      -- host module, one VM per app
local ui  = require("@agentos/ui")       -- constructors + helpers, generated from the component schema
```

**`app`**

| Call | Purpose |
|---|---|
| `app.manifest { id, name, icon, tint, window {w,h,minW,minH}, kit = ">=1.0", permissions }` | identity and minimum window (DESIGN_LANGUAGE: min 320x240) |
| `app.window(name, spec)` | declare a window kind. `spec`: `title`, `size`, `client = {name = {init, persist, sync, type}}`, `state = {...}` (per-instance server state), `render = function(w) -> Node`, `onOpen(w, params)`, `onClose(w)` |
| `app.state(initial)` | shared, in-memory server state; reads in `render` are tracked, assignments in handlers/tools schedule re-render of instances that read the field |
| `app.kv:get/set/delete` | small durable key-value in the app's SQLite |
| `app.db` | `migrate{[n]=sql}`, `exec(sql, params) -> {changes, last_insert_id}`, `all(sql, params)`, `one(sql, params)`, `tx(fn)`; **sources** (below) |
| `app.db:query(sql, {key, page, total})` | prepared once at load; returns a **factory**: calling it with params (values or Refs) gives a `rows` Source handle |
| `app.db:row(sql, {key, rev, write, conflict})` | single-row Source; `write(key, field, value, ctx)` makes fields two-way; `rev` names the revision column for conflict detection |
| `app.db:scalar(sql)` | scalar Source (badge counts) |
| `app.tool { name, title, description, input, readOnly, run(input, ctx) }` | an agent tool (MCP). `ctx.actor` (who), `ctx.now`, `ctx:fail(msg)`; writes made here update open views through sources (3.3) |
| `app.t.string / integer / number / boolean / array / object / enum` | input schema builders with `description`, `default`, `min`, `max` |
| `app:openWindow(name, params?)`, `app:windows(name)` | open or iterate window instances |
| `app:notify { title, body, at?, actions }` | Notification Centre (host feature) |
| `app.timer.at(time, fn)`, `app.timer.every(sec, fn)` | host-scheduled, VM-safe timers |

**`w` (window context passed to `render` and available in handler closures)**

| Member | Meaning |
|---|---|
| `w.state` | per-window server state; `w.state.x` is a **value** (tracked in render), assign in handlers |
| `w.client` | **Refs** to client-managed state: `w.client.search`; `w.client:get(name)` (only for `sync`/`persist` entries; handlers only), `w.client:set(name, v)` |
| `w.env` | Refs: `sizeClass`, `heightClass`, `locale`, `timeZone`, `scheme`, `now` |
| `w.id`, `w.params` | instance identity, deep-link params |
| `w:toast {text, icon, kind, action}`, `w:focus(key)`, `w:scrollTo(key)`, `w:announce(text)`, `w:copy(text)`, `w:close()`, `w:openWindow(...)` | one-shot `view.command`s |

**`ui` helpers**: expressions `ui.eq ne lt gt all any negate when(cond,a,b) concat coalesce len isEmpty upper lower add
sub`; formatting `ui.fmt.number currency percent bytes date time dateTime relative plural list`; actions `ui.act.set(ref,
value) toggle(ref) push(route) pop() popToRoot() focus(key) scrollTo(key) copy(text) emit(event, payload) seq{...}`;
`ui.each`. Row templates: `row = function(r) ... end` is called **once** at build time with a proxy `r` whose field reads
return Refs (`r.title` -> `row.title`). `tostring(ref)` and `"x" .. ref` raise "use ui.concat": a Ref has no value on the host.

**Handler argument** `e`: `e.key` (row key), `e.row` (host's last-sent row), `e.value`, `e.payload`, `e.actor`.

**Rules the host enforces.** `render` is pure (db writes and `app:*` calls raise inside render); one render per view
per frame; a Luau time slice per call (default 50 ms via the VM interrupt); interactive nodes need explicit keys;
icon-only controls need a `label`; every `Image` needs `alt` or `decorative = true`; only whitelisted props exist.
Errors carry the Luau `file:line` of the constructor call (dev builds).

**Typing.** The component schema (section 6) generates `ui.d.luau`; in `--!strict` a misspelled prop, a wrong enum value, a
`Binding` prop given a plain value, or an unknown slot is a `luau-analyze` error (wired into `bun run lint`), not a runtime surprise.

### 4.3 Example 1: Calculator

`calc.luau` is a straight port of `apps/calculator/src/shared/evaluate.ts` (`evaluate(expr): number` raising on
bad input, `format(n)`, `formatEntry(text)`); the app is below. State is **shared** (`app.state`): one calculator
however many windows are open, and the agent tool can drive it.

```lua
--!strict
local app  = require("@agentos/app")
local ui   = require("@agentos/ui")
local calc = require("./calc")
local t    = app.t

app.manifest {
  id = "calculator", name = "Calculator", icon = "plus", tint = "orange",
  window = { w = 340, h = 560, minW = 320, minH = 480 },   -- below 480 tall the keys fall under 44 px
}

-- One calculator for the whole app, however many windows are open.
local s = app.state {
  entry = "0", acc = 0, hasAcc = false, pending = "", fresh = true, caption = "", err = false,
}

local SYMBOL = { ["+"] = "+", ["-"] = "−", ["*"] = "×", ["/"] = "÷" }

-- ── Logic (pure Luau; no UI in here) ──────────────────────────────────────

local function apply(a: number, op: string, b: number): number
  s.caption = `{calc.format(a)} {SYMBOL[op]} {calc.format(b)}`
  return calc.evaluate(`{a} {op} ({b})`)                -- raises on division by zero
end

local function digit(d: string)
  if s.fresh then
    s.entry, s.fresh = (if d == "." then "0." else d), false
  elseif d == "." then
    if not string.find(s.entry, ".", 1, true) then s.entry ..= "." end
  elseif #string.gsub(s.entry, "[-.]", "") < 9 then
    s.entry = if s.entry == "0" then d else s.entry .. d
  end
end

local function operator(op: string)
  local value = tonumber(s.entry) :: number
  if s.hasAcc and s.pending ~= "" and not s.fresh then
    s.acc = apply(s.acc, s.pending, value)
    s.entry = tostring(s.acc)
  else
    s.acc, s.hasAcc = value, true
  end
  s.pending, s.fresh = op, true
end

local function equals()
  if not s.hasAcc or s.pending == "" then return end
  s.entry = tostring(apply(s.acc, s.pending, tonumber(s.entry) :: number))
  s.hasAcc, s.pending, s.fresh = false, "", true
end

local function clear()    -- "C" clears what is being typed; "AC" (nothing typed) clears everything
  if s.entry ~= "0" and not s.fresh then
    s.entry = "0"
  else
    s.entry, s.hasAcc, s.pending, s.caption = "0", false, "", ""
  end
  s.err, s.fresh = false, true
end

local actions: { [string]: (string?) -> () } = {
  digit = digit, operator = operator, equals = equals, clear = clear,
  negate = function()
    s.entry = if s.entry:sub(1, 1) == "-" then s.entry:sub(2) elseif s.entry == "0" then "0" else "-" .. s.entry
  end,
  percent = function() s.entry = tostring((tonumber(s.entry) :: number) / 100) end,
  backspace = function()
    if not s.fresh then s.entry = if #s.entry > 1 and s.entry ~= "-0" then s.entry:sub(1, -2) else "0" end
  end,
}

local function press(kind: string, arg: string?)
  if s.err and kind ~= "clear" then return end
  if not pcall(actions[kind], arg) then
    s.err, s.hasAcc, s.pending, s.fresh = true, false, "", true
  end
end

-- ── View ─────────────────────────────────────────────────────────────────

local function digitKey(d: string)
  return { id = d, name = d, label = d, kind = "digit", arg = d, face = "digit", shortcut = d }
end
local function opKey(id, symbol, arg, name)
  return { id = id, name = name, label = symbol, kind = "operator", arg = arg, face = "operator", shortcut = arg }
end

local KEYS = {
  { id = "clear", name = "Clear", label = "AC", kind = "clear", face = "function", shortcut = "escape" },
  { id = "negate", name = "Plus minus", label = "±", kind = "negate", face = "function" },
  { id = "percent", name = "Percent", label = "%", kind = "percent", face = "function", shortcut = "%" },
  opKey("div", "÷", "/", "Divided by"),
  digitKey("7"), digitKey("8"), digitKey("9"), opKey("mul", "×", "*", "Times"),
  digitKey("4"), digitKey("5"), digitKey("6"), opKey("sub", "−", "-", "Minus"),
  digitKey("1"), digitKey("2"), digitKey("3"), opKey("add", "+", "+", "Plus"),
  { id = "0", name = "0", label = "0", kind = "digit", arg = "0", face = "digit", shortcut = "0", span = 2 },
  { id = "dot", name = "Decimal point", label = ".", kind = "digit", arg = ".", face = "digit", shortcut = "." },
  { id = "eq", name = "Equals", label = "=", kind = "equals", face = "operator", shortcut = "enter" },
}
local FILL = { digit = "gray4", ["function"] = "gray2", operator = "orange" }   -- semantic palette, auto-contrast text

local function keyView(k)
  local title = k.label
  if k.id == "clear" and s.entry ~= "0" and not s.fresh then title = "C" end
  return ui.Button {
    key = "key-" .. k.id,
    title = title, label = k.name,
    shape = "circle", size = "fill", fill = FILL[k.face], span = k.span,
    selected = k.kind == "operator" and s.pending == k.arg and s.fresh,    -- aria-pressed on the pending operator
    shortcut = k.shortcut,
    onTap = function() press(k.kind, k.arg) end,
  }
end

app.window("main", {
  title = "Calculator",
  render = function(w)
    local display = if s.err then "Error" else calc.formatEntry(s.entry)
    return ui.Scope { scheme = "dark", background = "primary",    -- the instrument canvas: black body
      ui.VStack { spacing = 2, padding = 3,
        ui.Spacer {},
        ui.Text { key = "caption", s.caption, style = "body", foreground = "secondary", align = "trailing", lines = 1 },
        ui.Text { key = "display", display, label = "Result",
                  style = "display", fit = true, align = "trailing", lines = 1, live = "polite" },
        ui.Grid { columns = 4, spacing = 2, cellAspect = 1, ui.each(KEYS, keyView) },
        ui.Shortcut { key = "backspace", shortcut = "backspace", onTrigger = function() press("backspace") end },
      },
    }
  end,
})

-- ── Agent tools ──────────────────────────────────────────────────────────

local function record(expression: string, result: number)
  local h = app.kv:get("history") or {}
  table.insert(h, 1, { expression = expression, result = result })
  while #h > 50 do table.remove(h) end
  app.kv:set("history", h)
end

app.tool {
  name = "calculate", title = "Calculate", readOnly = true,
  description = "Evaluate an arithmetic expression exactly. Supports + - * / % ^, parentheses, unary minus and decimals.",
  input = { expression = t.string { description = 'Arithmetic expression, e.g. "(12.5 + 7) * 3"' } },
  run = function(input)
    local result = calc.evaluate(input.expression)
    record(input.expression, result)
    return { expression = input.expression, result = result, display = calc.format(result) }
  end,
}

app.tool {
  name = "show_on_calculator", title = "Show on Calculator",
  description = "Open the Calculator and show the result of an expression, so the user can keep calculating.",
  input = { expression = t.string {} },
  run = function(input)
    local result = calc.evaluate(input.expression)
    record(input.expression, result)
    s.entry, s.caption, s.hasAcc, s.pending, s.fresh, s.err =
      tostring(result), input.expression, false, "", true, false
    app:openWindow("main")        -- every open Calculator window re-renders by itself
    return { shown = calc.format(result) }
  end,
}

app.tool {
  name = "history", title = "Calculation History", readOnly = true,
  description = "List recent calculations, newest first.",
  input = { limit = t.integer { default = 10, min = 1, max = 50 } },
  run = function(input)
    local h = app.kv:get("history") or {}
    return { entries = table.move(h, 1, math.min(#h, input.limit), 1, {}) }
  end,
}
```

What this shows: all state and logic are ordinary Luau; the view is a pure function of `s`; no event plumbing for
the agent tool (compare the current app's `ctx.emit('display', ...)` + a client `api.on` listener); the palette is
semantic (`fill = "gray4"`), and the "black body" is `Scope { scheme = "dark" }`, not a hex. Pressing a key is one event
and one `set` op or two (2.8-A). The keypad costs a server round trip per key; on a local host that is far below a frame, see open question 1 for remote hosts.

### 4.4 Example 2: Notes

Folders sidebar, a notes list bound to a query with search, an editor with a two-way-bound `TextEditor` and a Markdown
preview, a delete confirmation `Alert`, and an agent tool `notes.append` that updates the open view live.

```lua
--!strict
local app = require("@agentos/app")
local ui  = require("@agentos/ui")
local t   = app.t
local db  = app.db

app.manifest {
  id = "notes", name = "Notes", icon = "note.text", tint = "yellow",
  window = { w = 960, h = 640, minW = 480, minH = 360 },
}

db:migrate {
  [1] = [[
    create table folders (
      id integer primary key, name text not null, position real not null default 0
    );
    create table notes (
      id integer primary key,
      folder_id integer not null references folders(id) on delete cascade,
      body text not null default '',
      rev integer not null default 0,                      -- bumped on every write: conflict detection
      updated_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
      author text                                          -- who wrote last: a person or an agent
    );
    create index notes_by_folder on notes(folder_id, updated_at desc);
    insert into folders(name) values ('Notes');
  ]],
}

-- ── Data: prepared once; the SQL never leaves the host ──────────────────────

local folders = db:query([[
  select f.id, f.name, (select count(*) from notes n where n.folder_id = f.id) as count
  from folders f order by f.position, f.id
]], { key = "id" })

local noteList = db:query([[
  select id,
         coalesce(nullif(trim(substr(body, 1, instr(body || char(10), char(10)) - 1)), ''), 'New Note') as title,
         substr(replace(body, char(10), ' '), 1, 120) as preview,
         updated_at
  from notes
  where (:folder = 0 or folder_id = :folder)
    and (:q = '' or body like '%' || :q || '%')
  order by updated_at desc, id desc                        -- total order: required for windowed lists
]], { key = "id", page = 60 })

local note = db:row([[select id, body, rev, updated_at, author from notes where id = :id]], {
  key = "id",
  rev = "rev",                                              -- compared with the client's baseRev on write-back
  write = function(id, field, value, ctx)                   -- the host calls this for a two-way bound field
    assert(field == "body")
    db:exec([[update notes set body = :v, rev = rev + 1, updated_at = :now, author = :who where id = :id]],
      { v = value, id = id, now = ctx.now, who = ctx.actor.name })
  end,
})

local function newNote(folder: number, who: string): number
  local target = if folder == 0 then 1 else folder
  return db:exec("insert into notes(folder_id, author) values (:f, :who)", { f = target, who = who }).last_insert_id
end

-- ── Window ─────────────────────────────────────────────────────────────────

app.window("main", {
  title = "Notes",

  client = {                                                -- client-managed: instant, no round trip
    folder        = { init = 0,      persist = "app" },     -- 0 = All Notes; survives closing the window
    noteId        = { init = 0,      persist = "app" },
    mode          = { init = "edit", persist = "app" },
    search        = { init = "",     persist = "session" },
    confirmDelete = { init = false },                       -- the alert; never persisted
    deleteId      = { init = 0,      sync = "commit" },     -- mirrored so the alert's handler can read it
  },

  render = function(w)
    local c = w.client                                      -- Refs, not values
    local current = note { id = c.noteId }                  -- live single-row source, parameterised by a client ref

    local function askDelete(id)                            -- an Action: runs on the client, then the Alert shows
      return ui.act.seq { ui.act.set(c.deleteId, id), ui.act.set(c.confirmDelete, true) }
    end

    return ui.Group {
      ui.SplitView { columns = 3,

        sidebar = ui.Screen { title = "Folders",
          ui.List { key = "folders", style = "sidebar", selection = c.folder,
            ui.Row { key = "all", tag = 0, icon = "tray.full", title = "All Notes" },
            ui.ForEach { source = folders {}, row = function(r)
              return ui.Row { tag = r.id, icon = "folder", title = r.name, badge = r.count }
            end },
          },
        },

        content = ui.Screen { title = "Notes", titleDisplay = "large",
          searchable = { text = c.search, placeholder = "Search", sync = "debounce:250" },
          toolbar = { trailing = ui.Button { key = "new-note", icon = "square.and.pencil", label = "New Note",
            shortcut = "cmd+n",
            onTap = function(e) w.client:set("noteId", newNote(w.client:get("folder"), e.actor.name)) end } },

          ui.List { key = "notes", style = "plain", selection = c.noteId,
            empty = ui.EmptyState { icon = "note.text", title = "No Notes",
              message = ui.when(ui.isEmpty(c.search), "Tap the compose button to start.", "No results.") },
            ui.ForEach { source = noteList { folder = c.folder, q = c.search }, row = function(r)
              return ui.Row { tag = r.id, title = r.title,
                subtitle = ui.concat(ui.fmt.relative(r.updated_at), "  ", r.preview),
                swipeTrailing = { ui.Button { key = "delete", icon = "trash", label = "Delete",
                  role = "destructive", onTap = askDelete(r.id) } },
              }
            end },
          },
        },

        detail = ui.Screen { titleDisplay = "inline",
          toolbar = {
            principal = ui.Picker { key = "mode", style = "segmented", selection = c.mode, label = "Mode",
              options = { { value = "edit", label = "Edit" }, { value = "preview", label = "Preview" } } },
            trailing = ui.Button { key = "delete-current", icon = "trash", label = "Delete Note",
              role = "destructive", disabled = ui.eq(c.noteId, 0), onTap = askDelete(c.noteId) },
          },

          ui.Show { when = ui.eq(c.noteId, 0),
            ui.EmptyState { icon = "note.text", title = "No Note Selected" },
            otherwise = ui.VStack { spacing = 0,
              ui.Show { when = ui.eq(c.mode, "edit"),
                -- two-way: the field shows what you type at once; the host gets one debounced write
                ui.TextEditor { key = "body", text = current:bind("body"), label = "Note",
                  placeholder = "Start writing…", sync = "debounce:400", grow = 1 },
                -- the preview reads the same ref: it follows every keystroke, with no round trip
                otherwise = ui.ScrollView { grow = 1,
                  ui.Markdown { key = "preview", text = current.body, padding = 4 } },
              },
              ui.Text { key = "meta", padding = 2, style = "footnote", foreground = "secondary",
                ui.concat("Edited ", ui.fmt.relative(current.updated_at),
                  ui.when(ui.isEmpty(current.author), "", ui.concat(" by ", current.author))) },
            },
          },
        },
      },

      ui.Alert { key = "confirm-delete", presented = c.confirmDelete,
        title = "Delete this note?", message = "This can’t be undone.",
        actions = {
          ui.Button { key = "confirm", title = "Delete", role = "destructive", onTap = function()
            local id = w.client:get("deleteId")
            db:exec("delete from notes where id = :id", { id = id })
            if w.client:get("noteId") == id then w.client:set("noteId", 0) end
          end },
          ui.Button { key = "cancel", title = "Cancel", role = "cancel" },   -- any action dismisses
        } },
    }
  end,
})

-- ── Agent tools ────────────────────────────────────────────────────────────

app.tool {
  name = "notes.search", title = "Search Notes", readOnly = true,
  description = "Find notes whose text contains the query. Returns id, title and a preview.",
  input = { query = t.string {}, limit = t.integer { default = 10, min = 1, max = 50 } },
  run = function(input)
    return db:all([[select id, substr(body, 1, 60) as title, updated_at from notes
                    where body like '%' || :q || '%' order by updated_at desc limit :n]],
                  { q = input.query, n = input.limit })
  end,
}

app.tool {
  name = "notes.append", title = "Append to Note",
  description = "Append text (Markdown) on a new line at the end of a note. If the note is open the user sees it appear live.",
  input = { id = t.integer { description = "Note id from notes.search" }, text = t.string {} },
  run = function(input, ctx)
    local r = db:exec([[
      update notes set body = body || char(10) || :text, rev = rev + 1, updated_at = :now, author = :who
      where id = :id]], { text = input.text, id = input.id, now = ctx.now, who = ctx.actor.name })
    if r.changes == 0 then return ctx:fail(`No note with id {input.id}`) end
    return { id = input.id }
  end,
}
```

Things the acceptance test is meant to show:

- **No glue for agent changes.** `notes.append` writes SQLite; `noteList` and `note` are live sources over `notes`, so the
  list row, preview line, folder count, editor text and "Edited 2s ago by Claude" all update (trace 2.8-B). If the user is
  typing in that note the dirty-field rules (3.4) merge instead of clobbering.
- **Two-way binding is one expression**: `text = current:bind("body")`. Debounce, drafts, `baseRev`, merge, caret
  mapping and the "Saving..." affordance come from the kit and the protocol, not from the app.
- **Presentation is client state.** The alert, the mode switch, the selection, the search text and the sheet-like
  detail all change at 0 ms; `ui.act.seq` opens the alert without the app being involved; the app only acts on the
  confirmed delete.
- **Search is a source param**: typing updates `client.search` locally, and 250 ms later the client asks the host for the
  new window; the app has no event handler for it.
- **The window survives a host restart and a closed window**: `folder`, `noteId`, `mode` are `persist = "app"`.

### 4.5 What the host sends for the Notes window (abridged)

```jsonc
// view.open -> ViewSnapshot
{ "viewId": "v1", "proto": 1, "kit": "1.0", "rev": 1,
  "clientDecls": { "folder": {"init":0,"persist":"app"}, "noteId": {"init":0,"persist":"app"},
                   "mode": {"init":"edit","persist":"app"}, "search": {"init":"","persist":"session"},
                   "confirmDelete": {"init":false}, "deleteId": {"init":0,"sync":"commit"} },
  "client": { "folder": 3, "noteId": 7, "mode": "edit" },                 // restored from the host's _ui_state
  "sources": {
    "s_folders": { "kind":"rows", "key":"id", "params":{} },
    "s_notes":   { "kind":"rows", "key":"id", "page":60, "debounce":250,
                   "params": { "folder":{"$ref":"client.folder"}, "q":{"$ref":"client.search"} } },
    "s_note":    { "kind":"row", "key":"id", "writable":["body"], "params":{"id":{"$ref":"client.noteId"}} } },
  "tree": { "type":"Group", "key":"main", "children":[
    { "type":"SplitView", "key":"_0", "props":{"columns":3}, "slots":{
        "sidebar":[ { "type":"Screen", "key":"_0", "props":{"title":"Folders"}, "children":[
          { "type":"List", "key":"folders", "props":{"style":"sidebar","selection":{"$ref":"client.folder"}}, "children":[
            { "type":"Row", "key":"all", "props":{"tag":0,"icon":"tray.full","title":"All Notes"} },
            { "type":"ForEach", "key":"_1", "props":{"source":"s_folders"}, "slots":{ "row":[
              { "type":"Row", "key":"_0", "props":{"tag":{"$ref":"row.id"},"icon":"folder",
                "title":{"$ref":"row.name"},"badge":{"$ref":"row.count"}} } ]} } ]} ]} ],
        "content":[ ... ],  "detail":[ ... ] } },
    { "type":"Alert", "key":"confirm-delete", "props":{"presented":{"$ref":"client.confirmDelete"},"title":"Delete this note?", ...},
      "slots":{"actions":[ {"type":"Button","key":"confirm","props":{"title":"Delete","role":"destructive"},"on":{"tap":true}},
                           {"type":"Button","key":"cancel","props":{"title":"Cancel","role":"cancel"}} ]} } ] },
  "data": { "s_folders": {...}, "s_notes": {"kind":"rows","total":214,"keys":[...],"rows":{...}},
            "s_note": {"kind":"row","rev":9,"row":{...}} } }
```

The whole window is ~3 KB of tree plus the first data window; the list template is sent once whether the folder holds 5
notes or 5 000. Test ids that fall out: `notes-folders-row-3`, `notes-new-note`, `notes-notes-row-7`,
`notes-body`, `notes-confirm-delete-confirm` (5.4).

### 4.6 Testing and sandbox

- **Host-side tests need no browser.** *(Planned, not built: today app logic is tested from Rust integration tests
  that load the real package, see `docs/TESTING.md`.)* `@agentos/test` mounts a window in the host and exposes the same ids as the
  e2e specs: `local v = test.mount(app, "main"); v:tap("calculator-key-7"); v:expectText("calculator-display", "7")`;
  `v:type("notes-body", "hi")`; `v:tree()` for snapshots (the tree is JSON). Tool tests call `app.tool` directly against a
  temp SQLite and assert the resulting `data.patch` for an open view.
- **Sandbox.** One Luau VM per app, `--!strict` encouraged, no `os`/`io`/`debug`, no `require` outside the app bundle
  and the three host modules, interrupt-based time slices, memory cap, per-app SQLite file, and capabilities (network,
  files, notifications) gated by `app.manifest.permissions` and the Settings permission model.
- **Dev loop**: saving a `.luau` file hot-reloads the VM; each open instance re-renders against the new code and the
  host sends the minimal ops (state is kept when the schema of `app.state` allows).


---

## 5. Theming, styling and accessibility

### 5.1 Can apps style? Only with semantic tokens.

**Decision: no raw styling.** An app can say *what a thing is* and *how much space it needs*, never *what colour or
font it is*. No CSS, no hex values for chrome, no arbitrary font sizes, no custom fonts, shadows, gradients, blur or
keyframes. The closed vocabulary:

| Concern | Allowed values | Maps to |
|---|---|---|
| Spacing (`padding`, `spacing`) | integers 0 to 8 = n x 4 pt, per-edge `{x,y,top,...}` | DESIGN_LANGUAGE s5 (4 pt grid) |
| Size (`frame`) | `{minW,idealW,maxW,minH,idealH,maxH}` in pt, `"fill"`; `aspect`, `cellAspect` | layout only; clamped to the window; readable-width caps are components' job |
| Type | `style`: `largeTitle title1 title2 title3 headline body callout subheadline footnote caption1 caption2` (+ `display`: the Calculator-class instrument readout, `fit`), `weight` regular..bold, `design default|rounded|mono` | `--type-*` tokens, Dynamic Type scales with Settings > Text Size |
| Foreground | `primary secondary tertiary tint destructive success warning` (+ `onFill` set by the kit) | `--label*`, `--tint`, `--ios-red/green/orange` |
| Background | `primary secondary tertiary grouped elevated fill fill2 fill3 fill4 tint tintSoft` | `--bg*`, `--bg-grouped*`, `--fill*`, `--tint-fill` |
| Tint (`tint`, `fill`) | system palette names `blue red green orange yellow purple pink indigo teal mint cyan brown gray gray2..gray6` | `--ios-*`; text on a `fill` gets **auto-contrast** (kit picks `onFill` to hold 4.5:1) |
| Corner | `none xs sm md lg xl full` | `--radius-*` |
| Border | `none hairline` | `--separator` |
| Elevation / material | **not settable**: determined by what a component is (Sheet, Popover, Alert, Menu get `--shadow-popover`; chrome gets `--material-*`) | DESIGN_LANGUAGE s3, s5 |
| Motion | `animation: default|none` | `--ease-ios`, `--duration-*`, honours Reduce Motion |
| Colour scheme | `Scope { scheme }` | `[data-theme]` subtree |

**Content colours are data, not styling.** User-chosen colours (a task label, a calendar's tint, a canvas fill, an
avatar) come as `ColorValue = palette token | "#rrggbb"` only in components whose purpose is user content: `ColorPicker`,
`Avatar`, `Chip`, `Canvas` shapes, `CalendarGrid` events, `Table` cell tints. The kit derives a readable foreground
(`onColor`) and clamps fills that would fail contrast in the current scheme.

**Escape hatches, in order of preference**: (1) a different semantic value; (2) `Scope { scheme, tint }` for instruments
(Calculator's black body, a terminal): this is the DESIGN_LANGUAGE "app's own canvas" exception, bounded to the dark/light
scheme and the palette; (3) `Canvas` for custom drawing; (4) ask for a **new token or component in the kit**: there is
no per-app extension point on purpose.

**Why so strict.** Every guarantee in DESIGN_LANGUAGE (light and dark parity, 4 pt grid, one accent, flat surfaces, AA
contrast, Reduce Transparency, text scaling) holds *by construction* when apps cannot opt out; the host can re-theme or
tighten the language later without touching apps; untrusted, agent-written apps cannot ship hostile or ugly CSS; and the wire
stays small and typed. The cost is flexibility; the kit's breadth (section 1) is what pays for it.

### 5.2 Who sets tokens and the theme

The host owns `tokens.css` (unchanged, `light-dark()` based) and sends session-wide `theme.set {scheme, tint,
textScale, reduceMotion, reduceTransparency, increaseContrast}` (Settings > Appearance, Accessibility); the client sets
`data-theme`, `--tint`, root `font-size`, `.reduce-transparency` exactly as today. Apps never see these values (they
read `w.env.scheme` only to branch *structure*, which is rare). `app.manifest.tint` is the app's default accent; the user's
accent overrides. Because a tree contains tokens and not colours, **a theme change repaints every window with zero
host work and zero messages**.

Adaptivity is client-side and size-class based: `w.env.sizeClass` is `compact` below 560 px container width (matches the
existing `SplitViewTemplate collapseAt` and the 700 px sidebar rule), `Adaptive`/`SplitView` switch locally while the
window is being resized.

### 5.3 Accessibility

The kit already targets WCAG 2.1 AA with axe in every story and desktop spec; SDUI keeps that target and moves most
enforcement to the **tree validator** (host, on every tree emitted, plus dev-time `luau-analyze` rules):

| Requirement | How it is met |
|---|---|
| **Names** | schema marks components `a11y.name: required`; the validator rejects an icon-only `Button`, a `Toggle` without text or `label`, an `Image` without `alt` (or `decorative`), a `TextField` without `label` or `placeholder` |
| **Roles** | implied by the component (`List` -> `list`, `Row` -> `listitem`, `Screen` -> `main`, `SplitView` sidebar -> `navigation`, `Alert` -> `alertdialog`, `TabView` -> `tablist`); `heading` on `Text` maps to `h1`..`h6` |
| **Values and state** | `Toggle.on` -> `aria-checked`, `Button.selected` -> `aria-pressed`, `Slider` -> `aria-valuenow/min/max/text`, `ProgressView` -> `progressbar`, disclosure -> `aria-expanded`, virtualised rows -> `aria-setsize`/`aria-posinset` from `total` |
| **Live regions** | `Text.live`, `view.command announce`, toast and error banners are `role=status`/`alert`; ack-adopted merges announce "Note updated"; list reorder and DnD announce position |
| **Focus order** | DOM order = tree order; there is no `tabindex` in the protocol. Containers with arrow-key semantics (`List`, `TabView`, segmented `Picker`, toolbars, menus) are one tab stop (roving tabindex); `autofocus` on at most one node per `Screen`/`Sheet` |
| **Focus management** | presentations trap focus, restore it to the invoking node (remembered **by path**, so it survives re-render), `Esc` dismisses; navigation push moves focus to the new screen's title, pop restores the previously focused row; after a `rows` patch removes the focused row focus moves to its neighbour |
| **Keyboard** | every `onTap` is operable with Space/Enter; `shortcut` props register window-scoped commands (and show in menus); type-to-select in lists/menus; DnD always has a non-pointer alternative (a `contextMenu` "Move to...", `Alt+Arrow`), enforced in review and by a validator warning on `drag` without a `contextMenu` sibling |
| **Contrast, targets** | colours are tokens or auto-contrasted data colours (5.1); kit minimum target 44 pt (`size = small` only inside toolbars, padded to 44 pt tappable area) |
| **Motion and scale** | Reduce Motion honoured by every transition; layouts survive 85 to 125 % text scale because sizes are tokens; `display` text uses `fit` |
| **Testing** | every catalogue story (generated from schema examples) runs through the existing axe + visual-snapshot spec in light and dark at min and preferred window sizes; host-side tree tests assert names/roles by id |

### 5.4 Test ids

Rule (implemented in `client/src/renderer/src/view/core/testid.ts`, tested): **`testid = app + "-" + the explicit keys on the path`**,
joined with `-`. The window root, **auto keys** (`_0`) and **slot names** are skipped (wrapping a node in a stack, or moving it into
a toolbar slot, never renames it); a row template contributes `row-<rowKey>` (the item's stable id, never its index
or text). An explicit `testid` prop overrides.

| Path | Derived id |
|---|---|
| `main/_0/_1/_2/key-7` | `calculator-key-7` (identical to today's id) |
| `main/actions/save` | `notes-actions-save` |
| `main/list/@row/item/delete`, rowKey 7 | `notes-list-row-7-item-delete`; the row container is `notes-list-row-7` |
| `main/confirm-delete/@actions/confirm` | `notes-confirm-delete-confirm` |
| overlay scrim (kit) | `<id>-scrim` |

Enforcement: the validator requires an explicit `key` on every interactive node (so ids cannot depend on position) and
checks **uniqueness per view** in dev, failing with "set `testid`"; kit adapters always pass the derived id to the kit
component's required `testid` prop, so `lint:testids` keeps passing unchanged. Existing journey specs port by id;
the only churn is where today's ids were ad hoc (the Calculator display is `calculator-display`, not `-display-value`,
unless the app sets `testid`).

---

## 6. The type pipeline

**As built: Rust first, not the YAML below.** The component schema is Rust (`crates/agentos-domain`, the `ui` module:
types plus the component catalogue tables); `agentos-codegen` turns it into `types/generated/agentos.ts` (protocol types
and the runtime `COMPONENTS` table), `agentos.d.luau`, `ui.luau`, `app.luau` and `docs/reference/components.md`.
`bun run codegen` writes them and `bun run codegen:check` (part of `bun run lint`) fails when they are stale; there is no
`gen:ui` or `schema:diff` command. The diagram below is the real flow; from "Authoring format" on, the text is the
original schema-first proposal (YAML authoring), kept to explain the shape of the outputs: its paths and command names
are not the real ones.

One component schema, generated outputs that are committed, and a no-diff check (the proposal asked for exactly this):

```
crates/agentos-domain, ui module   (Rust types + the component catalogue: tokens, Action, Expr, Source decls, event payloads)
        │   bun run codegen     (cargo run -p agentos-codegen; `bun run codegen:check` is the no-diff check)
        ├─▶ types/generated/agentos.ts        protocol types + the runtime COMPONENTS table  ──▶  client/src/renderer/src/view/protocol.ts
        ├─▶ types/generated/agentos.d.luau    the `app` API types (luau-lsp)
        ├─▶ types/generated/{ui,app}.luau     constructors (the Luau runtime's `@agentos/ui`) and the `@agentos/app` shim
        └─▶ docs/reference/components.md      the component reference
```

**Authoring format.** YAML whose property types are JSON Schema fragments, plus `x-agentos` metadata that no plain
type system carries (events with payloads, slots, a11y rules, `since`, tier, bindings, positional arg, kit mapping):

```yaml
# schema/components/TextField.yaml
name: TextField
since: "1.0"
tier: 1
interactive: true
a11y: { name: required, role: textbox }
positional: text                        # ui.TextField { "initial" } sets `text`
props:
  text:        { type: string, binding: text, doc: "The edited text. Two-way." }
  placeholder: { type: string, expr: true }
  kind:        { enum: [text, email, url, number, tel, search], default: text }
  validate:    { $ref: defs#/Validation }
  sync:        { $ref: defs#/SyncPolicy, default: "debounce:300" }
events:
  onSubmit:      { payload: { value: string } }
  onFocusChange: { payload: { focused: boolean } }
slots:
  prefix: { max: 1 }
  suffix: { max: 1 }
examples:
  - { title: Search, node: { type: TextField, key: q, props: { text: { $ref: client.search }, placeholder: Search } } }
maps: { adapter: TextField, kit: "@agentos/ui/molecules#TextFieldRow" }
```

**Generated Luau** (`ui.d.luau`): every prop, enum, `Binding` (typed `Ref<T>`, so a plain value in a `Binding` prop is a type error),
slot and event payload:

```lua
export type TextFieldProps = {
  key: string,                                   -- interactive nodes need an explicit key
  text: Ref<string>,                             -- binding
  placeholder: Expr<string>?,
  kind: ("text" | "email" | "url" | "number" | "tel" | "search")?,
  validate: Validation?, sync: SyncPolicy?,
  prefix: Node?, suffix: Node?,
  onSubmit: ((e: { value: string }) -> ())?,
  [string]: never,                                -- unknown props are errors
}
declare ui: { TextField: (props: TextFieldProps) -> Node, --[[ ... ]] }
```

`ui.luau` is generated too: defaults, positional mapping, a11y checks, auto-keys, `fallback` slots for post-1.0 components.

**Generated TypeScript**: `NodeOf<'TextField'>` types, the `Action`/`Expr`/`Op`/`Source*` protocol types (today hand-written in
`client/src/renderer/src/view/protocol.ts`, now re-exporting the generated types), and a runtime `COMPONENTS` table (`interactive`, `bindings`, `events`, `since`) the renderer uses
for `Unsupported` decisions and the binding-writability check.

**Rust**: `build.rs` embeds `kit.schema.json`; the host validates every emitted tree and every event payload against it
(compiled once with the `jsonschema` crate; full validation for the first tree and in debug, structural fast-path in
release). The host never needs per-component Rust structs: nodes are JSON values end to end, so adding a prop needs no Rust change.

**Contract tests (the "drift is a build failure" part)**, run by the repo's gate (`pre-commit` / `test`):

1. every schema component has an adapter, a generated story per example and an axe pass; every adapter prop appears in the schema;
2. Luau constructor snapshots: `ui.X {...}` of each example equals the schema's example node;
3. golden trees: Luau fixture app -> JSON -> `ViewCore` -> rendered DOM -> axe, for both example apps here;
4. (not built: `schema:diff <git-ref>` was to classify changes as additive, deprecation or breaking, and fail on breaking within a major; today `bun run codegen:check` only catches an unregenerated file);
5. the kit's `bun scripts/design-audit.ts` runs on every adapter.

**Why not the alternatives.** *Rust-first* (`schemars` + `ts-rs`/`specta`): attractive because validation equals the types, but
events/slots/a11y/`since`/docs need a proc-macro layer, Luau generation is custom either way, and every kit author (Svelte
developers) would have to edit Rust to add a prop. *Protobuf*: poor fit for JSON trees with open-ended `Expr` values. *TypeSpec* is a
reasonable alternative authoring syntax with a JSON Schema emitter; if the team prefers a typed DSL over YAML, swap the authoring
layer and keep the same four outputs.

---

## 7. The prototype (`proto/`)

Historical: `proto/` was the spike that proved the renderer core. It was removed once `client/src/renderer/src/view`
(`ViewCore` in `core/`, tested under `bun test`) replaced it, so the commands and file names below no longer exist.

Vite + Svelte 5, importing the real `@agentos/ui` atoms from the repo path (read-only), the host as a TypeScript stub on an
in-memory JSON-RPC wire (messages really serialised). `proto/README.md` has the run commands; summary:

```sh
cd proto && bun test src        # 28 tests: tree ops, expressions, merge, ViewCore protocol incl. drafts/sources/resync
node e2e/run.mjs                # headless Chromium e2e (starts and stops its own Vite); --shots writes screenshots
```

| Spec claim | Demonstrated |
|---|---|
| Node/path/ops, idempotence, resync on gap or broken address | `tree.test.ts`, `view.test.ts` |
| Render with kit components from JSON (`Stack`, `Text`, `Button`, `TextField`, `List`) | e2e + screenshots |
| Patch applied | `view.ops` for status/empty-state (keyed `insert`/`remove`/`set`), e2e |
| Event emitted with path + rowKey; derived test ids | e2e, `context.test.ts` |
| Two-way binding: instant draft, one debounced write, `maxWait`, dirty-field shield, host 3-way merge, ack adoption, flush before events | `view.test.ts`, `merge.test.ts`, e2e (agent `notes.append` while typing) |
| Client state as source params; client actions without round trips; row deltas | `view.test.ts`, e2e |

Deliberately not built: windowing, `row.set`, navigation, presentations, persistence/reconnect, Luau, the other components.
Finding from building it: the unit that must be pure and framework-free is **`ViewCore`** with injected storage (so Svelte
`$state` proxies make every mutation fine-grained and plain objects make it testable under `bun test`); adapters stay tiny
(10 to 40 lines each).

---

## 8. Plan and open questions

Historical: the original plan, written before the build. Milestone names and package paths (`packages/renderer`,
`gen:ui`) are not the real ones; `docs/architecture/ARCHITECTURE.md` has the waves as built.

### 8.1 Build order

| Milestone | Scope | Exit |
|---|---|---|
| **M0 Schema and conformance** | schema format, `codegen`, validator, protocol crate, golden fixtures | the two example apps' trees validate; generated TS/Luau compile |
| **M1 Core + Calculator** | Rust `ViewInstance` (render, reconcile, ops), Luau `ui` + tracked state, the client renderer (`ViewCore` productionised), Tier 1 layout/text/button/grid | Calculator ported, journeys green, p95 key -> pixels <= 30 ms |
| **M2 Data + Notes** | sources, live queries, drafts/merge, client state + persistence, `TextEditor`, `Markdown`, `List` windowing, `SplitView` x3, `Alert`, `Screen` search | Notes ported incl. `notes.append` live; reconnect and host-restart tests |
| **M3 Rest of Tier 1** | navigation, presentations, DnD, pickers, `Form`, swipe/reorder/multi-select; Tasks, Reminders, Contacts, Settings | each app passes the `agentos-code-review` readiness gate |
| **M4 Tier 2** | `Table`, `ChatTranscript`, `CalendarGrid`, `Outline`, `LazyGrid`; Agents, Calendar, Files, Books | |
| **M5 Tier 3** | `CodeEditor`, `DiffView`, `Canvas`; Coder, Reviewer, Diagrams | |

Each component lands with its schema entry, adapter, a story per state (generated from examples), axe in both schemes,
a host-side tree test, and a journey spec per app journey, i.e. the existing definition of done, unchanged.

### 8.2 Open questions for the user

1. **Remote hosts.** Is a host that is not on the same machine (VPS, another device) a supported topology? Every
   server-side event costs one round trip; typing, navigation and sheets do not (client-managed), but Calculator-class keypads
   do. If remote matters, options: accept it; allow a tiny **client-side action macro** for pure UI logic; or compile a
   Luau subset to run on the client (large scope). The design above assumes local or LAN.
2. **Host-side persistence of client UI state** (`persist = "app"`: selection, route, mode in `_ui_state`): acceptable
   as app data (backed up/synced with the app's SQLite)? Should the user be able to clear it per app in Settings?
3. **Several human clients on one window** (laptop plus tablet on the same view): the conflict contract covers one human
   plus agents. Two humans editing one note concurrently would want OT/CRDT for `text` sources. In scope now, or later?
4. **Heavy third-party engines in the client** (CodeMirror 6, xterm.js, a Markdown parser with syntax highlighting): are lazy-loaded
   chunks acceptable under the "no app code on the client" rule? (They are kit code, not app code.) Preferred libraries?
5. **Browser app (#12)**: what is a `WebView` in a managed renderer? Sandboxed `<iframe>` (cross-origin pages mostly refuse),
   or a host-driven remote browser streamed to a `Canvas`/video surface (matches "agents can see what I see" and
   permissioning, much more work). Needs a product decision before Tier 3 planning.
6. **Params convention**: new methods use by-name params objects; the current SDK peer uses positional arrays. Rewrite the peer
   (trivial) or wrap objects in one-element arrays?
7. **Schema authoring**: YAML + JSON Schema (proposed) vs TypeSpec vs Rust-first. Who writes new components day to day:
   Svelte developers (favours schema-first) or Rust developers?
8. **Shell-level declarations** (menu bar commands, window title/subtitle, dock badge, notifications' deep links, share
   targets): should they hang off `app.window` (`commands(w)`, `badge`) or a separate `app.shell` registry? Proposed:
   `w.commands` plus `app:notify`, rendered by the shell, with the same ids/a11y rules.
9. **i18n**: who owns strings? Proposed `app.t("key")` against per-app catalogues resolved on the host per `session.locale`,
   with ICU plurals and formatting done on the client (`fmt.plural`). Needs a decision before the first non-English app.
10. **Offline behaviour of the PWA**: dim and queue drafts (proposed) vs block UI vs full offline cache of sources (read-only)?
11. **Permissions**: UI-originated writes run as the user, tool writes as the agent (`ctx.actor`). Confirm that Settings
    permission policies apply at the tool and `app.db` capability layer (not per source), and that agents can be denied
    reading a source that the user can see.
12. **Revision history for conflicts** (`_ui_revisions`): retention (last N per field vs N days), and does it surface as a
    general "Undo agent change" affordance across apps (a shell feature) or stay a per-app toast?
13. **Fallback policy** for old clients: `Unsupported` placeholder plus per-node `fallback` (proposed) vs refusing to open an app
    whose `kit` is newer than the client.
14. **Window size classes**: confirm 560 px (container) for `compact`, versus the 700 px split-view collapse rule in
    DESIGN_LANGUAGE (the spec follows the existing 560 px `SplitViewTemplate` default and the 700 px sidebar rule separately).
