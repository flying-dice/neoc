/**
 * The client-side store of one view instance (one window). It is plain data: the Svelte layer hands the core a
 * deep `$state` proxy of this shape (`store.svelte.ts`) so every mutation is fine-grained; tests hand it plain
 * objects. Nothing in `core/` imports Svelte.
 *
 * The four state tiers of spec 3.1 map onto it:
 *  - ephemeral UI (scroll, caret, focus): in the adapters, never here;
 *  - client-managed: `client` (declared by the view, client-authoritative);
 *  - app state: `state` (server values by path) with `drafts` as the instant overlay of the user's edits;
 *  - data: `sources` (windows of live query results).
 */
import type { Json, Node, Row, RowKey, SizeClass } from '../protocol'

export interface RowsData {
  kind: 'rows'
  total: number
  /** Absolute position of `keys[0]` in the full result (the window start). */
  from: number
  keys: RowKey[]
  rows: Record<string, Row>
  /** A window request is in flight. */
  loading: boolean
}
export interface RowData {
  kind: 'row'
  row: Row | null
  rev: number
}
export interface ValueData {
  kind: 'value'
  value: Json
}
export type SourceData = RowsData | RowData | ValueData

export type NoticeKind = 'info' | 'success' | 'error' | 'conflict'
export interface Notice {
  id: number
  kind: NoticeKind
  text: string
}

/** `env.*` refs: the viewer's environment (spec 3.2). */
export interface ViewEnv {
  sizeClass: SizeClass
  heightClass: SizeClass
  locale: string
  timeZone: string
  scheme: 'light' | 'dark'
  /** Epoch ms, minute resolution. */
  now: number
}

export type LinkState = 'connecting' | 'online' | 'offline'
export type Phase = 'opening' | 'ready' | 'closed' | 'failed'

export interface ViewStore {
  tree: Node | null
  rev: number
  /** Server values of state paths (the latest the host sent, or the last ack). */
  state: Record<string, Json>
  /** The user's unacknowledged edits by binding key: resolved before the server value (spec 3.2). */
  drafts: Record<string, Json>
  /** Binding keys with an unsent or unacknowledged local edit ("Saving..."). */
  dirty: Record<string, boolean>
  /** Binding keys whose write was refused or overwritten elsewhere: the field's error text. */
  fieldErrors: Record<string, string>
  client: Record<string, Json>
  sources: Record<string, SourceData>
  env: ViewEnv
  link: LinkState
  phase: Phase
  /** `view.error` (the last good tree stays) or a failed open. */
  error: string | null
  /** `view.closed {reason}`: the instance is gone. */
  closed: string | null
  notices: Notice[]
  /** The latest polite live-region message ("Note updated", "Changed elsewhere"). */
  announcement: string
}

export function defaultEnv(): ViewEnv {
  const now = Math.floor(Date.now() / 60_000) * 60_000
  return {
    sizeClass: 'regular',
    heightClass: 'regular',
    locale: typeof navigator === 'undefined' ? 'en-GB' : navigator.language || 'en-GB',
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
    scheme: 'light',
    now
  }
}

export function plainStore(env: Partial<ViewEnv> = {}): ViewStore {
  return {
    tree: null,
    rev: 0,
    state: {},
    drafts: {},
    dirty: {},
    fieldErrors: {},
    client: {},
    sources: {},
    env: { ...defaultEnv(), ...env },
    link: 'connecting',
    phase: 'opening',
    error: null,
    closed: null,
    notices: [],
    announcement: ''
  }
}

export type StoreFactory = () => ViewStore

// ── Binding keys ───────────────────────────────────────────────────────────

/** What a two-way binding writes: a server state path, or a field of a bound row (spec 3.2 write table). */
export type BindingTarget =
  | { kind: 'state'; path: string }
  | { kind: 'row'; source: string; key: RowKey; field: string }

/** The stable key a target has in `drafts`, `dirty` and `fieldErrors`. */
export const bindingKey = (t: BindingTarget): string =>
  t.kind === 'state' ? `state:${t.path}` : `row:${t.source}:${String(t.key)}:${t.field}`
