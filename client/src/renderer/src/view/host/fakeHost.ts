/**
 * A TypeScript stand-in for the Rust host, for unit tests and Storybook. It speaks the real protocol over the
 * in-memory wire (JSON-RPC peer, by-name params) and plays the host's half of the contract:
 *
 *  - `session.hello`, `view.open` (fresh, resumed, or a fresh instance after a "host restart"), `view.sync`, `view.close`;
 *  - keyed ops with `base`/`rev` (an op log is not kept: a resume always answers with a snapshot);
 *  - per-binding `data.patch` and the host's conflict rules for `state.set` / `row.set` (three-way text merge on the
 *    base it last sent, human-wins scalars, `reject` mode), with a revision ring per path;
 *  - sources: `rows` windows with an ordered key list plus only the rows whose content hash changed, `row`, params
 *    from client refs, `source.set`;
 *  - events handed to the app, and "agent writes" (`setState`, `refreshSources`) that reach every open view.
 *
 * It is not a fake Luau runtime: an app here is a TypeScript `FakeApp` that returns trees and answers events.
 */
import { merge3 } from '../core/textMerge'
import type {
  ClientDecl,
  DataPatch,
  Expr,
  Json,
  Node,
  Op,
  Row,
  RowKey,
  SourceDecl,
  SourcePatch,
  ViewCommand,
  ViewEventParams,
  ViewSnapshot,
  WriteResolution,
  WriteResult
} from '../protocol'
import { AppErrorCode, KIT, PROTO } from '../protocol'
import { createRpcPeer, RpcError, type Params, type RpcPeer } from '../rpc/peer'
import type { ServerConnection } from './wire'

/** How the host resolves a write to a state path or row field when the writer's base is stale. */
export type MergeMode = 'text' | 'scalar' | 'reject'

export interface StateSpec {
  value: Json
  merge: MergeMode
}

/** Field writes to a source's rows (`row.set`): `write` applies one and returns the row's new revision. */
interface Writable {
  write?(key: RowKey, field: string, value: Json): number
  merge?: MergeMode
}
export interface RowsSourceSpec extends Writable {
  decl: SourceDecl & { kind: 'rows' }
  /** The full ordered result for these params (a total order). The host slices the window. */
  query(params: Record<string, Json>): Row[]
  find?(key: RowKey): Row | null
}
export interface RowSourceSpec extends Writable {
  decl: SourceDecl & { kind: 'row' }
  get(params: Record<string, Json>): Row | null
}
export type SourceSpec = RowsSourceSpec | RowSourceSpec

export interface FakeApp {
  id: string
  clientDecls: Record<string, ClientDecl>
  state: Record<string, StateSpec>
  sources: Record<string, SourceSpec>
  tree(view: FakeView): Node
  /** An event from the client; throw `new Error(msg)` for an `AppError`. */
  onEvent?(view: FakeView, event: ViewEventParams): void | Promise<void>
  /** Restore the app's data to its seed (a host restart). */
  reset?(): void
}

interface Ring {
  /** The values the host last sent per revision (the merge bases), newest last. */
  values: Map<number, Json>
}

const RING = 8
const hash = (row: Row): string => JSON.stringify(row)

export class FakeView {
  rev = 1
  conn: FakeConnection | null = null
  client: Record<string, Json> = {}
  /** Per source: the params and window the view asked for, and the rows it already has (by content hash). */
  sources = new Map<
    string,
    { params: Record<string, Json>; range: { from: number; to: number }; sent: Map<string, string> }
  >()
  windowId = ''
  /** Entries a restarted host adopted from the client's persisted values (they travel back in the snapshot). */
  adopted = new Set<string>()
  constructor(
    readonly id: string,
    readonly host: FakeHost
  ) {}

  /** Push a host notification to this view's client (dropped when it is disconnected, like a closed socket). */
  push(method: string, params: Params): void {
    this.conn?.peer.notify(method, { viewId: this.id, ...params })
  }

  /** Send keyed ops as one `view.ops` message and advance the revision. */
  ops(ops: Op[]): void {
    const base = this.rev
    this.rev++
    this.push('view.ops', { base, rev: this.rev, ops })
  }
  tree(tree: Node): void {
    this.rev++
    this.push('view.tree', { rev: this.rev, tree })
  }
  command(c: ViewCommand): void {
    this.push('view.command', c as unknown as Params)
  }
  setClient(path: string, value: Json, force = false): void {
    this.client[path] = value
    this.push('client.set', { path, value, force })
  }
}

class FakeConnection {
  readonly peer: RpcPeer
  readonly views = new Set<FakeView>()
  constructor(
    readonly host: FakeHost,
    conn: ServerConnection
  ) {
    this.peer = createRpcPeer({
      send: (m) => conn.send(m),
      onRequest: (method, params) => host.handle(this, method, params),
      onNotification: (method, params) => host.handleNotification(this, method, params),
      timeoutMs: 0
    })
    conn.onMessage((m) => this.peer.receive(m))
    conn.onClose(() => {
      this.peer.close()
      for (const v of this.views) if (v.conn === this) v.conn = null
    })
  }
}

export class FakeHost {
  private views = new Map<string, FakeView>()
  private entries = new Map<string, { value: Json; rev: number; merge: MergeMode; ring: Ring }>()
  private rowRings = new Map<string, Ring>()
  private revisions = new Map<number, { source: string; key: RowKey; field: string; value: Json }>()
  private nextRevision = 1
  private nextView = 1
  private sessionId = 's1'
  /** Everything the host received, for assertions. */
  readonly received: { method: string; params: Params }[] = []
  /** Writes that overlapped and resolved to the writer: kept for assertions (the real host keeps a revision ring). */
  readonly overwritten: { path: string; theirs: Json; mine: Json }[] = []

  constructor(readonly app: FakeApp) {
    this.seed()
  }

  private seed(): void {
    this.entries.clear()
    this.rowRings.clear()
    this.revisions.clear()
    for (const [path, spec] of Object.entries(this.app.state)) {
      this.entries.set(path, {
        value: spec.value,
        rev: 1,
        merge: spec.merge,
        ring: { values: new Map([[1, spec.value]]) }
      })
    }
  }

  /** Accept a connection from `createMemoryLink().onConnection`. */
  accept(conn: ServerConnection): void {
    new FakeConnection(this, conn)
  }

  /** A host restart: instances and state are gone; the next `session.hello` starts a fresh session. */
  restart(): void {
    this.views.clear()
    this.app.reset?.()
    this.seed()
    this.sessionId = `s${Number(this.sessionId.slice(1)) + 1}`
  }

  // ── Requests ───────────────────────────────────────────────────────────

  async handle(conn: FakeConnection, method: string, params: Params): Promise<unknown> {
    this.received.push({ method, params })
    switch (method) {
      case 'session.hello': {
        const resume = params.resume as { sessionId?: string } | undefined
        return {
          proto: PROTO,
          kit: KIT,
          hostVersion: 'fake-0.1',
          sessionId: this.sessionId,
          resumed: resume?.sessionId === this.sessionId
        }
      }
      case 'view.open':
        return this.open(conn, params)
      case 'view.sync':
        return this.snapshot(this.view(params))
      case 'view.close':
        this.views.delete(this.view(params).id)
        return null
      case 'state.set':
        return this.stateSet(
          this.view(params),
          params as { path: string; value: Json; baseRev: number }
        )
      case 'row.restore':
        return this.rowRestore(params as { source: string; key: RowKey; revisionId: number })
      case 'row.set':
        return this.rowSet(
          this.view(params),
          params as { source: string; key: RowKey; field: string; value: Json; baseRev: number }
        )
      case 'source.set':
        return this.sourceSet(
          this.view(params),
          params as {
            source: string
            params?: Record<string, Json>
            range?: { from: number; to: number }
          }
        )
      case 'view.event': {
        const view = this.view(params)
        try {
          await this.app.onEvent?.(view, params as unknown as ViewEventParams)
        } catch (error) {
          throw new RpcError(
            AppErrorCode.AppError,
            error instanceof Error ? error.message : String(error)
          )
        }
        return { ok: true }
      }
      default:
        throw new RpcError(-32601, `method not found: ${method}`)
    }
  }

  handleNotification(_conn: FakeConnection, method: string, params: Params): void {
    this.received.push({ method, params })
    if (method === 'client.state')
      Object.assign(this.view(params).client, params.set as Record<string, Json>)
  }

  private view(params: Params): FakeView {
    const view = this.views.get(String(params.viewId))
    if (!view) throw new RpcError(-32602, `no such view ${String(params.viewId)}`)
    return view
  }

  private open(conn: FakeConnection, params: Params): ViewSnapshot {
    const resume = params.resume as { viewId: string; client: Record<string, Json> } | undefined
    let view = resume ? this.views.get(resume.viewId) : undefined
    if (!view) {
      view = new FakeView(`v${this.nextView++}`, this)
      view.windowId = String(params.windowId)
      // A fresh instance adopts the client's persisted values for declared entries (spec 3.5).
      for (const [name, decl] of Object.entries(this.app.clientDecls)) {
        view.client[name] = resume?.client[name] ?? decl.init
        if (resume && name in resume.client) view.adopted.add(name)
      }
      this.views.set(view.id, view)
    }
    view.conn = conn
    conn.views.add(view)
    return this.snapshot(view)
  }

  // ── Snapshots, sources ─────────────────────────────────────────────────

  private evalParam(view: FakeView, expr: Expr): Json {
    if (typeof expr === 'object' && expr !== null && !Array.isArray(expr) && '$ref' in expr) {
      const [ns, name] = String(expr.$ref).split('.')
      if (ns === 'client') return view.client[name] ?? null
      if (ns === 'state') return this.entries.get(name)?.value ?? null
    }
    return expr as Json
  }

  private paramsFor(view: FakeView, spec: SourceSpec): Record<string, Json> {
    return Object.fromEntries(
      Object.entries(spec.decl.params ?? {}).map(([k, e]) => [k, this.evalParam(view, e)])
    )
  }

  private snapshot(view: FakeView): ViewSnapshot {
    const state: Record<string, { v: Json; r: number }> = {}
    for (const [path, e] of this.entries) state[path] = { v: e.value, r: e.rev }
    const sources: Record<string, SourceDecl> = {}
    const data: Record<string, SourcePatch> = {}
    view.sources.clear()
    for (const [id, spec] of Object.entries(this.app.sources)) {
      sources[id] = spec.decl
      const range = { from: 0, to: (spec.decl.kind === 'rows' ? spec.decl.page : undefined) ?? 60 }
      view.sources.set(id, { params: this.paramsFor(view, spec), range, sent: new Map() })
      data[id] = this.patchFor(view, id, true)
    }
    return {
      viewId: view.id,
      proto: PROTO,
      kit: KIT,
      rev: view.rev,
      tree: this.app.tree(view),
      state,
      // Only what the host restores: `persist = "app"` entries and values adopted on resume.
      client: Object.fromEntries(
        Object.entries(view.client).filter(
          ([name]) => this.app.clientDecls[name]?.persist === 'app' || view.adopted.has(name)
        )
      ),
      clientDecls: this.app.clientDecls,
      sources,
      data
    }
  }

  /** The patch bringing a view's copy of a source up to date: window keys plus only the changed rows. */
  private patchFor(view: FakeView, id: string, full = false): SourcePatch {
    const spec = this.app.sources[id]
    const st = view.sources.get(id)!
    if (spec.decl.kind === 'row') {
      const row = (spec as RowSourceSpec).get(st.params)
      if (row) this.noteSent(id, row[spec.decl.key ?? 'id'] as RowKey, row)
      return { kind: 'row', row, rev: typeof row?.rev === 'number' ? row.rev : 0 }
    }
    const result = (spec as RowsSourceSpec).query(st.params)
    const keyField = spec.decl.key ?? 'id'
    const window = result.slice(st.range.from, st.range.to)
    const rows: Record<string, Row> = {}
    const seen = new Set<string>()
    for (const row of window) {
      const k = String(row[keyField])
      seen.add(k)
      const h = hash(row)
      if (full || st.sent.get(k) !== h) {
        rows[k] = row
        this.noteSent(id, row[keyField] as RowKey, row)
      }
      st.sent.set(k, h)
    }
    const drop = [...st.sent.keys()].filter((k) => !seen.has(k))
    for (const k of drop) st.sent.delete(k)
    return {
      kind: 'rows',
      total: result.length,
      from: st.range.from,
      keys: window.map((r) => r[keyField] as RowKey),
      rows,
      drop
    }
  }

  /** Remember what a row looked like at its revision: the merge base of a later write (the ring of spec 3.4). */
  private noteSent(source: string, key: RowKey, row: Row): void {
    if (typeof row.rev !== 'number') return
    for (const [field, value] of Object.entries(row)) {
      const ringKey = `${source}:${String(key)}:${field}`
      const ring = this.rowRings.get(ringKey) ?? { values: new Map<number, Json>() }
      this.rowRings.set(ringKey, ring)
      this.remember(ring, row.rev, value)
    }
  }

  private sourceSet(
    view: FakeView,
    p: { source: string; params?: Record<string, Json>; range?: { from: number; to: number } }
  ): SourcePatch {
    const spec = this.app.sources[p.source]
    if (!spec) throw new RpcError(-32602, `no such source ${p.source}`)
    const st = view.sources.get(p.source)!
    if (p.params) st.params = p.params
    if (p.range) st.range = p.range
    else if (p.params)
      st.range = { from: 0, to: (spec.decl.kind === 'rows' ? spec.decl.page : undefined) ?? 60 }
    return this.patchFor(view, p.source)
  }

  /** Re-run every open view's sources and push what changed: what a SQLite commit hook does in the real host. */
  refreshSources(): void {
    for (const view of this.views.values()) {
      const sources: Record<string, SourcePatch> = {}
      for (const id of view.sources.keys()) sources[id] = this.patchFor(view, id)
      view.push('data.patch', { sources } as DataPatch as unknown as Params)
    }
  }

  // ── Writes and conflicts (spec 3.4) ────────────────────────────────────

  /** An agent (or any non-UI writer) sets a state path: every open view gets the new value and revision. */
  setState(path: string, value: Json, exceptView?: FakeView): number {
    const e = this.entries.get(path)
    if (!e) throw new Error(`no state path ${path}`)
    e.rev++
    e.value = value
    this.remember(e.ring, e.rev, value)
    for (const view of this.views.values())
      if (view !== exceptView)
        view.push('data.patch', { state: { [path]: { v: value, r: e.rev } } })
    return e.rev
  }

  stateValue(path: string): Json {
    return this.entries.get(path)?.value ?? null
  }

  private remember(ring: Ring, rev: number, value: Json): void {
    ring.values.set(rev, value)
    for (const k of [...ring.values.keys()].slice(0, -RING)) ring.values.delete(k)
  }

  /** Decide a write against the current value: the host's rule, by binding type. */
  private resolve(
    merge: MergeMode,
    ring: Ring,
    current: { value: Json; rev: number },
    baseRev: number,
    mine: Json,
    path: string
  ): { value: Json; resolution: WriteResolution; write: boolean; lost?: Json } {
    if (baseRev === current.rev) return { value: mine, resolution: 'accepted', write: true }
    if (merge === 'reject') return { value: current.value, resolution: 'rejected', write: false }
    if (merge === 'text' && typeof mine === 'string' && typeof current.value === 'string') {
      const base = ring.values.get(baseRev)
      const m = merge3(typeof base === 'string' ? base : current.value, current.value, mine)
      if (m.conflict) {
        this.overwritten.push({ path, theirs: current.value, mine })
        return { value: m.text, resolution: 'replaced', write: true, lost: current.value }
      }
      return { value: m.text, resolution: m.text === mine ? 'accepted' : 'merged', write: true }
    }
    if (JSON.stringify(current.value) === JSON.stringify(mine))
      return { value: mine, resolution: 'accepted', write: true }
    // scalar: the human wins; the old value is kept as a revision
    this.overwritten.push({ path, theirs: current.value, mine })
    return { value: mine, resolution: 'replaced', write: true, lost: current.value }
  }

  private stateSet(view: FakeView, p: { path: string; value: Json; baseRev: number }): WriteResult {
    const e = this.entries.get(p.path)
    if (!e) throw new RpcError(-32602, `unknown state path ${p.path}`)
    const r = this.resolve(e.merge, e.ring, e, p.baseRev, p.value, p.path)
    if (r.write) {
      e.rev++
      e.value = r.value
      this.remember(e.ring, e.rev, r.value)
      for (const other of this.views.values())
        if (other !== view)
          other.push('data.patch', { state: { [p.path]: { v: r.value, r: e.rev } } })
    }
    return {
      rev: e.rev,
      value: r.value,
      resolution: r.resolution,
      ...(r.resolution === 'replaced' || r.resolution === 'rejected' ? { conflict: true } : {})
    }
  }

  private rowSet(
    view: FakeView,
    p: { source: string; key: RowKey; field: string; value: Json; baseRev: number }
  ): WriteResult {
    const spec = this.app.sources[p.source]
    if (!spec?.write) throw new RpcError(-32602, `source ${p.source} is not writable`)
    const st = view.sources.get(p.source)!
    const row =
      spec.decl.kind === 'row'
        ? (spec as RowSourceSpec).get(st.params)
        : ((spec as RowsSourceSpec).find?.(p.key) ?? null)
    if (!row) throw new RpcError(-32602, `no row ${String(p.key)}`)
    const ringKey = `${p.source}:${String(p.key)}:${p.field}`
    const ring = this.rowRings.get(ringKey) ?? { values: new Map<number, Json>() }
    this.rowRings.set(ringKey, ring)
    const rev = typeof row.rev === 'number' ? row.rev : 0
    const r = this.resolve(
      spec.merge ?? 'scalar',
      ring,
      { value: row[p.field] ?? null, rev },
      p.baseRev,
      p.value,
      ringKey
    )
    let newRev = rev
    if (r.write) {
      newRev = spec.write(p.key, p.field, r.value)
      this.remember(ring, newRev, r.value)
      this.refreshSources()
    }
    const undo =
      r.resolution === 'replaced' && r.lost !== undefined ? this.keepRevision(p, r.lost) : undefined
    return {
      rev: newRev,
      value: r.value,
      resolution: r.resolution,
      ...(r.resolution === 'replaced' || r.resolution === 'rejected' ? { conflict: true } : {}),
      ...(undo ? { undo } : {})
    }
  }

  /** The value a human-wins write replaced, kept so `row.restore` can bring it back (spec 3.4 rule 8). */
  private keepRevision(
    p: { source: string; key: RowKey; field: string },
    lost: Json
  ): { revisionId: number; lost: Json } {
    const revisionId = this.nextRevision++
    this.revisions.set(revisionId, { source: p.source, key: p.key, field: p.field, value: lost })
    return { revisionId, lost }
  }

  private rowRestore(p: { source: string; key: RowKey; revisionId: number }): WriteResult {
    const kept = this.revisions.get(p.revisionId)
    const spec = this.app.sources[p.source]
    if (!kept || !spec?.write || kept.source !== p.source || String(kept.key) !== String(p.key))
      throw new RpcError(-32602, `no revision ${p.revisionId}`)
    const rev = spec.write(p.key, kept.field, kept.value)
    this.refreshSources()
    return { rev, value: kept.value, resolution: 'accepted' }
  }

  // ── Helpers for apps and tests ─────────────────────────────────────────

  allViews(): FakeView[] {
    return [...this.views.values()]
  }

  /** The `rev` and value the host last sent for a state path. */
  stateRev(path: string): number {
    return this.entries.get(path)?.rev ?? 0
  }
}
