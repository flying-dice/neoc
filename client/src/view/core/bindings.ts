/**
 * Two-way bindings: drafts, write-back, shielding and conflict handling.
 *
 * Every editable control binds a ref and follows the same small state machine per bound path, implemented
 * once here and used by every adapter:
 *
 *   clean --edit--> dirty --debounce/maxWait/blur/submit/event--> inflight --ack--> clean
 *                    ^  |                                            |
 *                    +--+--edit while inflight: stays dirty, next write is rebased on the ack
 *
 * Rules implemented:
 *  1. the user's value shows at once: `store.drafts[key]` overlays the server value; the control is never a
 *     server-controlled input;
 *  2. a dirty or inflight field shields itself: a remote change is applied to the server value but the draft
 *     keeps showing; `baseRev` does not advance, so the eventual write is merged by the host on the right base;
 *  3. write-back is debounced with a `maxWait`, or immediate (`change`), or on `blur` / `submit`; IME
 *     composition never flushes mid-composition; nothing is sent while offline (the draft is queued and
 *     replayed on reconnect);
 *  4. the write carries `baseRev`;
 *  5. the ack carries the authoritative value: adopted (and the caret mapped through the diff); if the user kept
 *     typing, the draft is rebased onto it;
 *  6. nothing is silently lost: an overlap that replaced a change made elsewhere raises a conflict notice; a
 *     refused write adopts the server value, announces "changed elsewhere" and puts the field in its error state.
 * Scalar controls are human-wins: the host returns the user's value, and a value that changed elsewhere in the
 * meantime is overwritten (the host keeps it in the revision ring).
 */
import type { Json, ResumeDraft, RowKey, WriteResult } from '../protocol'
import { isConnectionError } from '../rpc/peer'
import { bindingKey, type BindingTarget, type ViewStore } from './store'
import { rebase } from './textMerge'
import { realTimers, type TimerHandle, type Timers } from './timers'

// ── Sync policy ────────────────────────────────────────────────────────────

export type SyncPolicy =
  | { mode: 'debounce'; debounceMs: number; maxWaitMs: number }
  | { mode: 'change' }
  | { mode: 'blur' }
  | { mode: 'submit' }

export const TEXT_SYNC: SyncPolicy = { mode: 'debounce', debounceMs: 300, maxWaitMs: 1000 }
export const SCALAR_SYNC: SyncPolicy = { mode: 'change' }

/** Parse the `sync` prop: "debounce:300", "change", "blur", "submit". Anything else falls back to `fallback`. */
export function parseSync(spec: unknown, fallback: SyncPolicy): SyncPolicy {
  if (typeof spec !== 'string') return fallback
  const [mode, arg] = spec.split(':')
  if (mode === 'change' || mode === 'blur' || mode === 'submit') return { mode }
  if (mode === 'debounce') {
    const ms = Number(arg)
    const debounceMs = Number.isFinite(ms) && ms >= 0 ? ms : 300
    return { mode: 'debounce', debounceMs, maxWaitMs: Math.max(debounceMs, 1000) }
  }
  return fallback
}

// ── Acks ───────────────────────────────────────────────────────────────────

/** The ack of `state.set` / `row.set`: the host says how it resolved the write. */
export type WriteAck = WriteResult

/**
 *  - accepted: the value stands;
 *  - merged: a non-overlapping change from elsewhere was composed in;
 *  - replaced: an overlap resolved to the user's value (`undo` names the revision that lost);
 *  - rejected: the host refused the write (`conflict = "reject"` sources).
 */

export const sameJson = (a: Json | undefined, b: Json | undefined): boolean =>
  a === b || JSON.stringify(a) === JSON.stringify(b)

// ── Dependencies ───────────────────────────────────────────────────────────

export interface BindingsDeps {
  store: ViewStore
  online(): boolean
  /** Send `state.set` / `row.set` for a target. Rejects with a connection error when the socket is gone. */
  send(target: BindingTarget, value: Json, baseRev: number): Promise<WriteAck>
  /** The server value and revision the store holds for a target. */
  read(target: BindingTarget): { value: Json; rev: number } | undefined
  /** Write a server value for a target into the store. */
  adopt(target: BindingTarget, value: Json, rev: number): void
  /** `undo` is set when a row write replaced a change made elsewhere: the notice offers to bring it back. */
  notify(kind: 'info' | 'conflict' | 'error', text: string, undo?: UndoOffer): void
  announce(text: string): void
  onError(error: unknown): void
  timers?: Timers
}

/** What the notice for a replaced row write offers: restore the revision that lost. */
export interface UndoOffer {
  source: string
  key: RowKey
  field: string
  revisionId: number
}

/** A caret-mapping hook: called when an ack replaced what the user typed with the host's merged value. */
export type AdoptListener = (change: { before: Json; after: Json }) => void

interface Entry {
  target: BindingTarget
  key: string
  baseValue: Json
  /** The revision the draft is based on (what `baseRev` carries). Does not advance while dirty. */
  baseRev: number
  /** The newest server revision seen for this path (a remote change held behind the draft). */
  latestRev: number
  dirty: boolean
  inflight: boolean
  editSeq: number
  policy: SyncPolicy
  composing: boolean
  timer?: TimerHandle
  firstEditAt?: number
  promise?: Promise<void>
}

export class Bindings {
  private entries = new Map<string, Entry>()
  private listeners = new Map<string, Set<AdoptListener>>()
  private readonly timers: Timers

  constructor(private d: BindingsDeps) {
    this.timers = d.timers ?? realTimers
  }

  // ── Reading ────────────────────────────────────────────────────────────

  /** True while the binding has an unsent or unacknowledged local edit. */
  isDirty(target: BindingTarget): boolean {
    return this.entries.get(bindingKey(target))?.dirty === true
  }

  /** Subscribe to ack adoptions of one binding (the text field maps its caret through them). */
  onAdopt(target: BindingTarget, listener: AdoptListener): () => void {
    const key = bindingKey(target)
    const set = this.listeners.get(key) ?? new Set()
    set.add(listener)
    this.listeners.set(key, set)
    return () => void set.delete(listener)
  }

  // ── Writing ────────────────────────────────────────────────────────────

  private ensure(target: BindingTarget): Entry {
    const key = bindingKey(target)
    let e = this.entries.get(key)
    if (!e) {
      const known = this.d.read(target)
      e = {
        target,
        key,
        baseValue: known?.value ?? null,
        baseRev: known?.rev ?? 0,
        latestRev: known?.rev ?? 0,
        dirty: false,
        inflight: false,
        editSeq: 0,
        policy: TEXT_SYNC,
        composing: false
      }
      this.entries.set(key, e)
    }
    return e
  }

  /** The user edited a bound control. The control already shows `value`; we reconcile in the background. */
  edit(target: BindingTarget, value: Json, policy: SyncPolicy): void {
    const e = this.ensure(target)
    const { store } = this.d
    delete store.fieldErrors[e.key]
    e.policy = policy
    if (!e.inflight && sameJson(value, e.baseValue)) {
      // Typed and deleted back to what the host has: nothing to write.
      this.timers.clearTimeout(e.timer)
      e.dirty = false
      e.firstEditAt = undefined
      delete store.drafts[e.key]
      delete store.dirty[e.key]
      return
    }
    store.drafts[e.key] = value
    store.dirty[e.key] = true
    e.dirty = true
    e.editSeq++
    this.schedule(e)
  }

  private schedule(e: Entry): void {
    this.timers.clearTimeout(e.timer)
    if (e.composing) return
    switch (e.policy.mode) {
      case 'change':
        void this.flush(e.key)
        return
      case 'debounce': {
        const now = this.timers.now()
        e.firstEditAt ??= now
        const wait = Math.max(
          0,
          Math.min(e.policy.debounceMs, e.policy.maxWaitMs - (now - e.firstEditAt))
        )
        e.timer = this.timers.setTimeout(() => void this.flush(e.key), wait)
        return
      }
      case 'blur':
      case 'submit':
        return // flushed by `commit`, an event, close, or reconnect
    }
  }

  /** IME composition: nothing flushes mid-composition; ending it schedules the write. */
  setComposing(target: BindingTarget, composing: boolean): void {
    const e = this.ensure(target) // composition starts before the first `input` event
    e.composing = composing
    if (!composing && e.dirty) this.schedule(e)
  }

  /** Blur or submit: write now. */
  commit(target: BindingTarget): Promise<void> {
    const e = this.entries.get(bindingKey(target))
    if (!e) return Promise.resolve()
    e.composing = false
    return this.flush(e.key)
  }

  /** Send a dirty draft now. Resolves when the host has acknowledged it (or it could not be sent). */
  flush(key: string): Promise<void> {
    const e = this.entries.get(key)
    if (!e) return Promise.resolve()
    this.timers.clearTimeout(e.timer)
    if (e.inflight) return e.promise ?? Promise.resolve()
    if (!e.dirty || e.composing || !this.d.online()) return Promise.resolve()
    e.firstEditAt = undefined
    e.inflight = true
    const sentSeq = e.editSeq
    const sent = this.d.store.drafts[key]
    e.promise = (async () => {
      let followUp = false
      try {
        const ack = await this.d.send(e.target, sent, e.baseRev)
        followUp = this.acknowledge(e, sent, sentSeq, ack)
      } catch (error) {
        // Offline: the draft stays queued and is replayed on reconnect. A refusal keeps it too, with the error shown.
        if (!isConnectionError(error)) {
          this.d.store.fieldErrors[key] = 'Could not be saved'
          this.d.onError(error)
        }
      } finally {
        e.inflight = false
      }
      if (followUp) this.schedule(e)
    })()
    return e.promise
  }

  /** Flush every dirty draft and wait until the host has them (before an event, on close). */
  async settle(): Promise<void> {
    for (let i = 0; i < 5; i++) {
      const pending = [...this.entries.values()].filter((e) => e.dirty || e.inflight)
      if (pending.length === 0 || !this.d.online()) return
      await Promise.all(pending.map((e) => this.flush(e.key)))
    }
  }

  /** Replay every queued draft (after a reconnect), each with the `baseRev` it was written on. */
  replay(): Promise<void> {
    return this.settle()
  }

  /** What is still unacknowledged, for `view.open {resume}`. */
  pending(): ResumeDraft[] {
    return [...this.entries.values()]
      .filter((e) => e.dirty)
      .map((e) => {
        const value = this.d.store.drafts[bindingKey(e.target)] ?? null
        return e.target.kind === 'state'
          ? { kind: 'state', path: e.target.path, value, baseRev: e.baseRev }
          : {
              kind: 'row',
              source: e.target.source,
              key: e.target.key,
              field: e.target.field,
              value,
              baseRev: e.baseRev
            }
      })
  }

  // ── The ack ────────────────────────────────────────────────────────────

  /** Settle an ack into the store. Returns true when the user kept typing and a follow-up write must be scheduled. */
  private acknowledge(e: Entry, sent: Json, sentSeq: number, ack: WriteAck): boolean {
    const { store } = this.d
    const resolution = ack.resolution
    // A remote change that landed after the host applied our write is newer than the ack: keep it.
    const remoteNewer = e.latestRev > ack.rev
    if (!remoteNewer) {
      this.d.adopt(e.target, ack.value, ack.rev)
      e.latestRev = ack.rev
      e.baseValue = ack.value
    } else e.baseValue = this.d.read(e.target)?.value ?? ack.value
    e.baseRev = Math.max(ack.rev, e.latestRev)

    const typedMore = e.editSeq !== sentSeq && resolution !== 'rejected'
    if (typedMore) {
      // The user kept typing in flight: the draft stays, rebased on the ack so neither side's content is lost.
      const draft = store.drafts[e.key]
      if (typeof sent === 'string' && typeof ack.value === 'string' && typeof draft === 'string')
        store.drafts[e.key] = rebase(sent, ack.value, draft)
    } else {
      const before = store.drafts[e.key] ?? sent
      delete store.drafts[e.key]
      delete store.dirty[e.key]
      e.dirty = false
      const after = this.d.read(e.target)?.value ?? ack.value
      if (!sameJson(before, after))
        for (const l of this.listeners.get(e.key) ?? []) l({ before, after })
    }

    switch (resolution) {
      case 'merged':
        this.d.announce('Updated with a change made elsewhere')
        break
      case 'replaced':
        this.d.notify(
          'conflict',
          'Your edit replaced a change made elsewhere.',
          ack.undo && e.target.kind === 'row'
            ? {
                source: e.target.source,
                key: e.target.key,
                field: e.target.field,
                revisionId: ack.undo.revisionId
              }
            : undefined
        )
        break
      case 'rejected':
        store.fieldErrors[e.key] = 'Changed elsewhere'
        this.d.announce('Changed elsewhere. Your edit was not applied.')
        break
      case 'accepted':
        break
    }
    return typedMore
  }

  // ── Remote changes ─────────────────────────────────────────────────────

  /**
   * The host pushed a value for a target (`data.patch.state`, or a snapshot on open, resync and resume). A dirty
   * field shields itself: the server value updates, the draft keeps showing, and `baseRev` stays put until the ack.
   * A snapshot is authoritative even at a lower revision (a restarted host starts its revisions over); a live patch
   * that is not newer than what we hold is stale or an echo of our own write and is ignored.
   */
  remote(
    target: BindingTarget,
    value: Json,
    rev: number,
    mode: 'patch' | 'snapshot' = 'patch'
  ): void {
    const e = this.entries.get(bindingKey(target))
    const snapshot = mode === 'snapshot'
    if (!e) {
      const known = this.d.read(target)
      if (!snapshot && known && rev <= known.rev) return
      this.d.adopt(target, value, rev)
      return
    }
    if (e.dirty || e.inflight) {
      if (snapshot || rev > e.latestRev) {
        this.d.adopt(target, value, rev)
        e.latestRev = rev
      }
      return
    }
    if (!snapshot && rev <= e.baseRev) return
    this.d.adopt(target, value, rev)
    e.baseValue = value
    e.baseRev = rev
    e.latestRev = rev
  }

  /** A row patch arrived (already applied): track revisions for every binding on that row. */
  remoteRow(
    source: string,
    rowKey: string | number,
    row: Record<string, Json> | null,
    rev: number
  ): void {
    for (const e of this.entries.values()) {
      if (
        e.target.kind !== 'row' ||
        e.target.source !== source ||
        String(e.target.key) !== String(rowKey)
      )
        continue
      const field = row?.[e.target.field] ?? null
      if (e.dirty || e.inflight) e.latestRev = Math.max(e.latestRev, rev)
      else if (rev > e.baseRev || !sameJson(e.baseValue, field)) {
        e.baseValue = field
        e.baseRev = rev
        e.latestRev = rev
      }
    }
  }

  /** Drop every entry (the view closed or its source went away). */
  dispose(): void {
    for (const e of this.entries.values()) this.timers.clearTimeout(e.timer)
    this.entries.clear()
    this.listeners.clear()
  }
}
