/**
 * Sources: data bound to live host queries (spec 3.3, 3.7).
 *
 * The tree references a source by an opaque id; the SQL never leaves the host and the client sends only param
 * values. This module owns, per view:
 *  - the declarations and the data windows (`rows`: total + ordered keys + only the changed rows; `row`; `value`);
 *  - params as refs: when `client.search` (or any ref a param reads) changes, the client waits the decl's
 *    `debounce` and sends `source.set {source, params}`; stale replies are dropped by a per-source sequence number;
 *  - windows: `setRange` / `loadMore` ask for the rows the list needs (coalesced to a frame); rows outside the
 *    resident window are evicted past a cap;
 *  - the offline rule: a request that cannot be sent is remembered and re-run when the socket is back.
 *
 * `text` and `thread` sources (CodeEditor documents, chat transcripts) are tier 2/3 and not implemented here:
 * their patches are reported and ignored.
 */
import type { Expr, Json, Row, RowKey, SourceDecl, SourcePatch, SourceSetParams } from '../protocol'
import { isConnectionError } from '../rpc/peer'
import type { RowsData, ViewStore } from './store'
import { realTimers, type TimerHandle, type Timers } from './timers'

export const DEFAULT_PAGE = 60
/** Rows kept resident per source, in pages: the window plus one page each side and a little slack. */
const CACHE_PAGES = 5
/** Window requests are coalesced to a frame. */
const FRAME_MS = 16

export interface SourcesDeps {
  store: ViewStore
  online(): boolean
  /** `source.set` for this view (the caller adds `viewId`). */
  request(params: Omit<SourceSetParams, 'viewId'>): Promise<SourcePatch>
  /** Evaluate a param expression against the live store (reads are where reactivity is tracked). */
  evaluate(expr: Expr): unknown
  /** A row (or a single-row source) was written into the store: bindings track its revision. */
  onRow?(source: string, key: RowKey, row: Row | null, rev: number): void
  onError(error: unknown): void
  timers?: Timers
}

interface Range {
  from: number
  to: number
}

export class Sources {
  private decls: Record<string, SourceDecl> = {}
  private lastParams = new Map<string, string>()
  private ranges = new Map<string, Range>()
  private seqs = new Map<string, number>()
  private paramTimers = new Map<string, TimerHandle>()
  private rangeTimers = new Map<string, TimerHandle>()
  private pendingRange = new Map<string, Range>()
  private stale = new Set<string>()
  private warned = new Set<string>()
  private readonly timers: Timers

  constructor(private d: SourcesDeps) {
    this.timers = d.timers ?? realTimers
  }

  declaration(id: string): SourceDecl | undefined {
    return this.decls[id]
  }
  page(id: string): number {
    return this.decls[id]?.page ?? DEFAULT_PAGE
  }

  /** The row key field of a source (`key`, default `id`). */
  keyField(id: string): string {
    return this.decls[id]?.key ?? 'id'
  }

  // ── Install and patch ──────────────────────────────────────────────────

  /**
   * Install declarations and the first data windows (open, resync, resume). On a resume, sources whose params
   * changed while offline are re-queried after the snapshot lands.
   */
  load(decls: Record<string, SourceDecl>, data: Record<string, SourcePatch>, resumed: boolean): void {
    this.decls = decls
    for (const id of Object.keys(this.d.store.sources)) if (!(id in decls)) delete this.d.store.sources[id]
    for (const [id, patch] of Object.entries(data)) this.apply(id, patch)
    for (const id of Object.keys(decls)) {
      if (!resumed || !this.lastParams.has(id)) this.lastParams.set(id, this.paramsKey(id))
      if (!this.ranges.has(id)) this.ranges.set(id, { from: 0, to: this.page(id) })
    }
    if (resumed) this.refreshStale()
  }

  /** A `data.patch` for a source (or a reply to `source.set`). */
  apply(id: string, patch: SourcePatch): void {
    const { store } = this.d
    switch (patch.kind) {
      case 'row': {
        store.sources[id] = { kind: 'row', row: patch.row, rev: patch.rev }
        const key = patch.row?.[this.keyField(id)]
        if (key !== undefined && key !== null) this.d.onRow?.(id, key as RowKey, patch.row, patch.rev)
        return
      }
      case 'value':
        store.sources[id] = { kind: 'value', value: patch.value }
        return
      case 'rows':
        return this.applyRows(id, patch)
      default:
        if (!this.warned.has(id)) {
          this.warned.add(id)
          this.d.onError(new Error(`source "${id}": kind "${patch.kind}" is not supported by this client`))
        }
    }
  }

  private applyRows(id: string, patch: Extract<SourcePatch, { kind: 'rows' }>): void {
    const { store } = this.d
    let data = store.sources[id]
    if (data?.kind !== 'rows') {
      store.sources[id] = { kind: 'rows', total: patch.total, from: patch.from ?? 0, keys: patch.keys, rows: {}, loading: false }
      data = store.sources[id]
    }
    const rows = data as RowsData
    for (const [k, row] of Object.entries(patch.rows ?? {})) {
      rows.rows[k] = row
      this.d.onRow?.(id, k, row, typeof row.rev === 'number' ? row.rev : 0)
    }
    for (const k of patch.drop ?? []) delete rows.rows[String(k)]
    rows.keys = patch.keys
    rows.total = patch.total
    rows.from = patch.from ?? rows.from
    rows.loading = false
    this.evict(id, rows)
  }

  /** Keep the resident window plus a bounded cache of recently seen rows. */
  private evict(id: string, data: RowsData): void {
    const cap = this.page(id) * CACHE_PAGES
    const all = Object.keys(data.rows)
    if (all.length <= cap) return
    const resident = new Set(data.keys.map(String))
    let over = all.length - cap
    for (const k of all) {
      if (over <= 0) break
      if (resident.has(k)) continue
      delete data.rows[k]
      over--
    }
  }

  // ── Params are refs ────────────────────────────────────────────────────

  private paramsOf(id: string): Record<string, Json> {
    const decl = this.decls[id]
    const out: Record<string, Json> = {}
    for (const [name, expr] of Object.entries(decl?.params ?? {})) out[name] = (this.d.evaluate(expr) ?? null) as Json
    return out
  }
  private paramsKey = (id: string): string => JSON.stringify(this.paramsOf(id))

  /**
   * Re-query every source whose params changed. Call after anything a param may read changed (client state,
   * state patches, `client.set`). Typing in a search box lands here once per keystroke; the decl's `debounce`
   * collapses them into one `source.set`.
   */
  sync(): void {
    for (const id of Object.keys(this.decls)) {
      const key = this.paramsKey(id)
      if (key === this.lastParams.get(id)) continue
      this.lastParams.set(id, key)
      this.ranges.set(id, { from: 0, to: this.page(id) }) // a new query starts at the top
      this.pendingRange.delete(id)
      this.timers.clearTimeout(this.paramTimers.get(id))
      this.paramTimers.set(
        id,
        this.timers.setTimeout(() => void this.run(id), this.decls[id].debounce ?? 250)
      )
    }
  }

  // ── Windows ────────────────────────────────────────────────────────────

  range(id: string): Range {
    return this.ranges.get(id) ?? { from: 0, to: this.page(id) }
  }

  /** The list needs rows `[from, to)`: coalesced to a frame, skipped if already requested. */
  setRange(id: string, from: number, to: number): void {
    const cur = this.range(id)
    if (from === cur.from && to === cur.to) return
    this.pendingRange.set(id, { from, to })
    this.timers.clearTimeout(this.rangeTimers.get(id))
    this.rangeTimers.set(
      id,
      this.timers.setTimeout(() => {
        const next = this.pendingRange.get(id)
        this.pendingRange.delete(id)
        if (!next) return
        this.ranges.set(id, next)
        void this.run(id)
      }, FRAME_MS)
    )
  }

  /** Infinite load: widen the window by one page when the viewport nears the end. */
  loadMore(id: string): void {
    const data = this.d.store.sources[id]
    if (data?.kind !== 'rows' || data.loading) return
    if (this.pendingRange.has(id)) return // one widening per frame: wait for the window to land
    const pending = this.range(id)
    if (pending.to >= data.total) return
    this.setRange(id, pending.from, pending.to + this.page(id))
  }

  hasMore(id: string): boolean {
    const data = this.d.store.sources[id]
    return data?.kind === 'rows' && data.from + data.keys.length < data.total
  }

  // ── Requests ───────────────────────────────────────────────────────────

  /** Ask the host for the current params and window. Stale replies are dropped by sequence number. */
  private async run(id: string): Promise<void> {
    const decl = this.decls[id]
    if (!decl) return
    if (!this.d.online()) {
      this.stale.add(id)
      return
    }
    const seq = (this.seqs.get(id) ?? 0) + 1
    this.seqs.set(id, seq)
    const data = this.d.store.sources[id]
    if (data?.kind === 'rows') data.loading = true
    try {
      const patch = await this.d.request({
        source: id,
        params: this.paramsOf(id),
        ...(decl.kind === 'rows' ? { range: this.range(id) } : {})
      })
      if (this.seqs.get(id) !== seq) return // a newer request superseded this reply
      this.stale.delete(id)
      this.apply(id, patch)
    } catch (error) {
      const current = this.d.store.sources[id]
      if (current?.kind === 'rows') current.loading = false
      if (isConnectionError(error)) this.stale.add(id)
      else this.d.onError(error)
    }
  }

  /** Re-run the requests that could not be sent while offline. */
  refreshStale(): void {
    for (const id of [...this.stale]) void this.run(id)
  }

  dispose(): void {
    for (const t of [...this.paramTimers.values(), ...this.rangeTimers.values()]) this.timers.clearTimeout(t)
    this.paramTimers.clear()
    this.rangeTimers.clear()
    this.pendingRange.clear()
  }
}
