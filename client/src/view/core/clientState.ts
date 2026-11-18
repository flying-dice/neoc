/**
 * Client-managed state (spec 3.1, 3.5): the entries a view declares (`client = {name = {init, persist, sync}}`),
 * authoritative on the client. Reads and writes cost no round trip. This module owns:
 *
 *  - initial values: `init`, then the `session` copy kept in `sessionStorage` by window, then whatever the host
 *    restored (`persist = "app"` values arrive in the snapshot's `client`);
 *  - mirroring to the host (`client.state`, FIFO ahead of any later event) for `sync`/`persist` entries, queued
 *    while offline and flushed when the socket is back;
 *  - the app driving state (`client.set`): applied at once, except on a focused text control with a different
 *    value unless `force`.
 *
 * What the entries mean to the rest of the view (source params re-query, drafts) is wired by the caller through
 * `onChange`.
 */
import type { ClientDecl, Json } from '../protocol'
import type { ViewStore } from './store'
import { realTimers, type TimerHandle, type Timers } from './timers'

export interface KeyValueStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

/** An in-memory storage (tests, private windows where `sessionStorage` throws). */
export function memoryStorage(): KeyValueStorage {
  const map = new Map<string, string>()
  return {
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k)
  }
}

export interface ClientStateDeps {
  store: ViewStore
  /** Where `persist = "session"` entries live (keyed by window id). */
  storage?: KeyValueStorage
  windowId: string
  online(): boolean
  /** `client.state` notification (the caller adds `viewId`). */
  mirror(set: Record<string, Json>): void
  /** An entry changed: source params may need re-querying. */
  onChange(): void
  timers?: Timers
}

/** `commit` mirrors are coalesced; `change` mirrors go out on the next tick. */
const COMMIT_MS = 150

const nameOf = (path: string): string => path.split('.')[0]

function setIn(root: Json, path: string[], value: Json): Json {
  if (path.length === 0) return value
  const base = root !== null && typeof root === 'object' && !Array.isArray(root) ? root : {}
  return {
    ...base,
    [path[0]]: setIn((base as Record<string, Json>)[path[0]] ?? null, path.slice(1), value)
  }
}

export class ClientState {
  private decls: Record<string, ClientDecl> = {}
  private pending = new Map<string, Json>()
  private focused = new Set<string>()
  private mirrorTimer: TimerHandle | undefined
  private readonly timers: Timers

  constructor(private d: ClientStateDeps) {
    this.timers = d.timers ?? realTimers
  }

  private get storageKey(): string {
    return `agentos.view.${this.d.windowId}.client`
  }

  private effectiveSync(name: string): 'none' | 'change' | 'commit' {
    const decl = this.decls[name]
    if (!decl) return 'none'
    if (decl.sync && decl.sync !== 'none') return decl.sync
    return decl.persist === 'session' || decl.persist === 'app' ? 'commit' : 'none' // persist implies at least commit
  }

  private readSession(): Record<string, Json> {
    try {
      const raw = this.d.storage?.getItem(this.storageKey)
      return raw ? (JSON.parse(raw) as Record<string, Json>) : {}
    } catch {
      return {}
    }
  }

  private writeSession(): void {
    if (!this.d.storage) return
    const out: Record<string, Json> = {}
    for (const [name, decl] of Object.entries(this.decls))
      if (decl.persist === 'session' && name in this.d.store.client)
        out[name] = this.d.store.client[name]
    try {
      this.d.storage.setItem(this.storageKey, JSON.stringify(out))
    } catch {
      /* storage full or blocked: the entry simply does not survive a reload */
    }
  }

  /** Install declarations and initial values (open, resync, resume). Values already held locally win. */
  load(decls: Record<string, ClientDecl>, restored: Record<string, Json>): void {
    this.decls = decls
    const session = this.readSession()
    const { client } = this.d.store
    for (const [name, decl] of Object.entries(decls)) {
      if (name in client) continue
      client[name] =
        name in restored
          ? restored[name]
          : decl.persist === 'session' && name in session
            ? session[name]
            : decl.init
    }
    for (const [name, value] of Object.entries(restored))
      if (!(name in client) && !(name in decls)) client[name] = value
  }

  /** Values to hand the host on resume: what a restarted host adopts (spec 3.5 reconnect). */
  snapshot(): Record<string, Json> {
    const out: Record<string, Json> = {}
    for (const [name, decl] of Object.entries(this.decls)) {
      const persisted = decl.persist === 'session' || decl.persist === 'app'
      if ((persisted || this.effectiveSync(name) !== 'none') && name in this.d.store.client)
        out[name] = this.d.store.client[name]
    }
    return out
  }

  /** A write from the user or a client action. */
  set(path: string, value: Json): void {
    this.assign(path, value)
    const name = nameOf(path)
    if (this.effectiveSync(name) !== 'none') this.queueMirror(name)
    this.d.onChange()
  }

  /** `client.set` from the app. Returns false when a focused text control kept its own value. */
  hostSet(path: string, value: Json, force = false): boolean {
    const name = nameOf(path)
    if (
      !force &&
      this.focused.has(name) &&
      JSON.stringify(this.d.store.client[name]) !== JSON.stringify(value)
    )
      return false
    this.assign(path, value)
    this.d.onChange()
    return true
  }

  private assign(path: string, value: Json): void {
    const [name, ...rest] = path.split('.')
    const { client } = this.d.store
    client[name] = rest.length ? setIn(client[name] ?? null, rest, value) : value
    if (this.decls[name]?.persist === 'session') this.writeSession()
  }

  /** A text control bound to `client.<name>` gained or lost focus (the `client.set` guard). */
  setFocused(name: string, focused: boolean): void {
    if (focused) this.focused.add(name)
    else this.focused.delete(name)
  }

  private queueMirror(name: string): void {
    this.pending.set(name, this.d.store.client[name])
    this.timers.clearTimeout(this.mirrorTimer)
    const wait = this.effectiveSync(name) === 'change' ? 0 : COMMIT_MS
    this.mirrorTimer = this.timers.setTimeout(() => this.flushMirror(), wait)
  }

  /** Send queued mirrors now (before an event, on close, after a reconnect). Kept queued while offline. */
  flushMirror(): void {
    this.timers.clearTimeout(this.mirrorTimer)
    if (this.pending.size === 0 || !this.d.online()) return
    const set = Object.fromEntries(this.pending)
    this.pending.clear()
    this.d.mirror(set)
  }

  dispose(): void {
    this.timers.clearTimeout(this.mirrorTimer)
    this.pending.clear()
  }
}
