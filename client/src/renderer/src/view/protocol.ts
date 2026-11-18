/**
 * Wire types of the AgentOS view protocol (proto 1, kit 1.0).
 * Source of truth: docs/architecture/ui-system.md, sections 2 (view protocol) and 3 (bindings and state).
 *
 * TO BE REPLACED BY GENERATED TYPES. Wave A2 generates TypeScript from the Rust schema; this hand-written
 * module is the interim. It is the ONLY place that declares wire shapes: nothing else in the renderer
 * defines a message, node, op, patch or action type, so the swap is "delete this file, point the imports at
 * the generated module" (names and field spellings follow the spec exactly). The runtime constants and
 * the method tables at the bottom are part of the swap too.
 */

export const PROTO = 1
export const KIT = '1.0'

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json }

// ── Expressions (spec 3.2) ─────────────────────────────────────────────────

/** A reference into a binding scope: client.* | state.* | src.<id>[.field] | row.* | route.* | env.* */
export interface Ref {
  $ref: string
}
/** A call of a function from the closed set (spec 3.2). */
export interface Fn {
  $fn: string
  args?: Expr[]
}
/** A literal object that happens to contain `$ref` or `$fn` as its own key. */
export interface Lit {
  $lit: Json
}
export type Expr = Json | Ref | Fn | Lit

// ── Actions and handlers (spec 3.5) ────────────────────────────────────────

export type Action =
  | { do: 'set'; ref: string; value: Expr }
  | { do: 'toggle'; ref: string }
  | { do: 'nav.push'; stack?: string; route: { name: string; [param: string]: Expr } }
  | { do: 'nav.pop' }
  | { do: 'nav.popToRoot' }
  | { do: 'focus'; key: string }
  | { do: 'scrollTo'; key: string }
  | { do: 'copy'; text: Expr }
  | { do: 'emit'; event?: string; payload?: Record<string, Expr> }
  | { do: 'seq'; actions: Action[] }

/** `true` = send the event to the app; an Action[] runs on the client (`emit` entries also go to the app). */
export type Handler = true | Action[]

// ── Nodes and ops (spec 2.1, 2.2) ──────────────────────────────────────────

export interface Node {
  type: string
  key: string
  props?: Record<string, Expr>
  children?: Node[]
  slots?: Record<string, Node[]>
  on?: Record<string, Handler>
}

export type Op =
  | { op: 'set'; path: string; props: Record<string, Expr | null> }
  | { op: 'on'; path: string; on: Record<string, Handler | null> }
  | { op: 'insert'; parent: string; slot?: string; before: string | null; node: Node }
  | { op: 'remove'; path: string }
  | { op: 'move'; path: string; before: string | null }
  | { op: 'replace'; path: string; node: Node }

// ── State, client state, sources (spec 2.3, 3.3, 3.5) ──────────────────────

export type RowKey = string | number
export type Row = Record<string, Json>

/** A state value with its revision (`data.patch.state`, `ViewSnapshot.state`). */
export interface StateValue {
  v: Json
  r: number
}

export type SourceKind = 'rows' | 'row' | 'value' | 'text' | 'thread'

/** The SQL stays on the host: the client knows an opaque id and param expressions. */
export interface SourceDecl {
  kind: SourceKind
  key?: string
  page?: number
  /** Ms to wait after a param changes before asking the host. */
  debounce?: number
  writable?: string[]
  total?: 'exact' | 'estimate' | 'none'
  params?: Record<string, Expr>
}

export interface ClientDecl {
  init: Json
  persist?: 'none' | 'session' | 'app'
  sync?: 'none' | 'change' | 'commit'
  type?: Json
}

export type SourcePatch =
  | { kind: 'rows'; total: number; from?: number; keys: RowKey[]; rows?: Record<string, Row>; drop?: RowKey[] }
  | { kind: 'row'; rev: number; row: Row | null }
  | { kind: 'value'; value: Json }
  | { kind: 'text'; rev: number; length: number; value?: string }
  | { kind: 'thread'; rows?: Record<string, Row>; append?: { id: RowKey; path: string; text: string }[] }

export interface ViewSnapshot {
  viewId: string
  proto: number
  kit: string
  rev: number
  tree: Node
  state?: Record<string, StateValue>
  client: Record<string, Json>
  clientDecls: Record<string, ClientDecl>
  sources: Record<string, SourceDecl>
  data: Record<string, SourcePatch>
}

// ── Client -> host requests (spec 2.6) ─────────────────────────────────────

export interface WindowSize {
  w: number
  h: number
}

/** A pending local edit, declared on resume so the host knows what will be replayed (values are replayed by writes). */
export type ResumeDraft =
  | { kind: 'state'; path: string; baseRev: number }
  | { kind: 'row'; source: string; key: RowKey; field: string; baseRev: number }

export interface ViewOpenParams {
  app: string
  window: string
  windowId: string
  size: WindowSize
  params?: Record<string, Json>
  resume?: { viewId: string; rev: number; client: Record<string, Json>; drafts: ResumeDraft[] }
}

export interface SessionHelloParams {
  proto: number
  kit: string
  locale: string
  timeZone: string
  features: string[]
  resume?: { sessionId: string; views: { viewId: string; rev: number }[] }
}
export interface SessionHelloResult {
  proto: number
  kit: string
  hostVersion: string
  sessionId: string
  resumed: boolean
}

export interface ViewEventParams {
  viewId: string
  rev: number
  seq: number
  path: string
  rowKey?: RowKey
  event: string
  payload?: Json
}
export type ViewEventResult = { ok: true } | { ok: false; reason: string }

export interface StateSetParams {
  viewId: string
  path: string
  value: Json
  baseRev: number
}
export interface RowSetParams {
  viewId: string
  source: string
  key: RowKey
  field: string
  value: Json
  baseRev: number
}
/** The ack of a two-way write: the authoritative value and revision (spec 3.4 rule 6). */
export interface WriteResult {
  rev: number
  value: Json
  conflict?: boolean
}

export interface SourceSetParams {
  viewId: string
  source: string
  params?: Record<string, Json>
  range?: { from: number; to: number }
  anchor?: 'start' | 'end'
}

/** Client -> host requests: method -> params and result. */
export interface RequestMethods {
  'session.hello': { params: SessionHelloParams; result: SessionHelloResult }
  'view.open': { params: ViewOpenParams; result: ViewSnapshot }
  'view.sync': { params: { viewId: string }; result: ViewSnapshot }
  'view.event': { params: ViewEventParams; result: ViewEventResult }
  'state.set': { params: StateSetParams; result: WriteResult }
  'row.set': { params: RowSetParams; result: WriteResult }
  'source.set': { params: SourceSetParams; result: SourcePatch }
  'view.close': { params: { viewId: string }; result: null }
}

/** Client -> host notifications (spec 2.6). */
export interface ClientNotifications {
  'client.state': { viewId: string; set: Record<string, Json> }
  'view.resize': { viewId: string; size: WindowSize; sizeClass: SizeClass }
  'view.visibility': { viewId: string; visible: boolean }
  'session.bye': Record<string, never>
}

// ── Host -> client notifications (spec 2.6) ────────────────────────────────

export type SizeClass = 'compact' | 'regular'

export interface DataPatch {
  viewId: string
  state?: Record<string, StateValue>
  sources?: Record<string, SourcePatch>
}

export type ViewCommand =
  | { cmd: 'toast'; args: { text: string; icon?: string; kind?: 'info' | 'success' | 'error'; duration?: number } }
  | { cmd: 'focus'; args: { key: string } }
  | { cmd: 'scrollTo'; args: { key?: string; row?: RowKey } }
  | { cmd: 'announce'; args: { text: string } }
  | { cmd: 'copy'; args: { text: string } }
  | { cmd: 'openUrl'; args: { url: string } }
  | { cmd: 'haptic'; args: Record<string, never> }
  | { cmd: 'closeWindow'; args: Record<string, never> }

export interface ThemeSet {
  scheme: 'light' | 'dark'
  tint: string
  textScale: number
  reduceMotion: boolean
  reduceTransparency: boolean
  increaseContrast: boolean
}

/** Host -> client notifications: method -> params. */
export interface HostNotifications {
  'view.tree': { viewId: string; rev: number; tree: Node }
  'view.ops': { viewId: string; base: number; rev: number; ops: Op[] }
  'data.patch': DataPatch
  'client.set': { viewId: string; path: string; value: Json; force?: boolean }
  'view.command': { viewId: string } & ViewCommand
  'view.error': { viewId: string; message: string; dev?: string }
  'view.closed': { viewId: string; reason: string }
  'theme.set': ThemeSet
}

/** JSON-RPC application error codes (spec 2.4). */
export const AppErrorCode = {
  /** The handler raised an error whose message is safe to show. */
  AppError: -32010
} as const

export type RequestMethod = keyof RequestMethods
export type HostNotificationMethod = keyof HostNotifications
