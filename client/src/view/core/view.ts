/**
 * The client-side core of one view instance (one window): the contract of the protocol in code, framework-free.
 * The Svelte layer hands it a `$state` store and renders `store.tree` through the component registry; tests hand
 * it plain objects. It runs no app code: it applies the host's tree, ops and patches, resolves refs through the
 * closed expression language, and owns the interaction rules of spec section 3:
 *
 *  - client-managed state is authoritative here; source params observe it (`ClientState`, `Sources`);
 *  - two-way bindings are drafts with debounced write-back, shielding and conflict handling (`Bindings`);
 *  - events address nodes by key path and flush dirty drafts first;
 *  - tree ops are keyed and versioned (`base` -> `rev`); a gap or a broken address asks for `view.sync`;
 *  - offline: the view stays mounted, drafts and state writes queue, events are refused, and a reconnect resumes.
 */
import type {
  Action,
  ClientNotifications,
  DataPatch,
  Expr,
  Handler,
  HostNotifications,
  Json,
  Node,
  Row,
  RowKey,
  RequestMethod,
  RequestMethods,
  ViewCommand,
  ViewEventResult,
  ViewOpenParams,
  ViewSnapshot,
  WindowSize
} from '../protocol'
import { AppErrorCode } from '../protocol'
import { messageOf } from '../errors'
import { isConnectionError, RpcError } from '../rpc/peer'
import {
  Bindings,
  parseSync,
  SCALAR_SYNC,
  TEXT_SYNC,
  type AdoptListener,
  type SyncPolicy
} from './bindings'
import { ClientState, type KeyValueStorage } from './clientState'
import { dig, evaluate, isRef, type FormatEnv } from './expr'
import { NavRegistry } from './nav'
import { Sources } from './sources'
import {
  bindingKey,
  plainStore,
  type BindingTarget,
  type Notice,
  type NoticeKind,
  type StoreFactory,
  type ViewStore
} from './store'
import { ShortcutRegistry } from './shortcuts'
import { realTimers, type Timers } from './timers'
import { applyOps, findByKey, MAX_DEPTH, MAX_NODES, measure, OpError } from './tree'

/** What the view needs from the session: requests, notifications and whether the socket is up. */
export interface ViewLink {
  request<M extends RequestMethod>(
    method: M,
    params: RequestMethods[M]['params']
  ): Promise<RequestMethods[M]['result']>
  notify<M extends keyof ClientNotifications>(method: M, params: ClientNotifications[M]): void
  readonly online: boolean
}

/** Imperative one-shots the host (or a client action) can ask for; the Svelte layer implements them on the DOM. */
export interface ViewEffects {
  focus?(path: string): void
  scrollTo?(path: string, rowKey?: RowKey): void
  copy?(text: string): void
  /**
   * Whether the person has just tapped or pressed a key (a transient user activation). The host pushes `view.command
   * copy` at any time, so the clipboard is only written while this says yes. Left out (tests, stories): always.
   */
  userActive?(): boolean
  openUrl?(url: string): void
  closeWindow?(): void
  haptic?(): void
}

/** Where a node renders: inside a row template it sees `row.*`; inside a route destination `route.*`. */
export interface Scope {
  row?: Row
  rowKey?: RowKey
  /** The source the row came from (so `row.<field>` bindings know where to write). */
  source?: string
  route?: Record<string, Json>
}

export interface ViewCoreOptions {
  link: ViewLink
  store?: StoreFactory
  windowId: string
  storage?: KeyValueStorage
  timers?: Timers
  effects?: ViewEffects
  onError?: (error: unknown) => void
  /** The instance id was assigned or changed (open, or a fresh instance after a host restart). */
  onViewId?: (viewId: string, previous: string) => void
}

export interface Binding {
  readonly ref: string
  /** The effective value: draft overlay first, then the server (or client) value. */
  readonly value: unknown
  readonly dirty: boolean
  readonly error: string | undefined
  /** Which tier the ref lives in: `client` writes are instant and local; `server` ones are drafts with write-back. */
  readonly tier: 'client' | 'server'
  set(value: Json, policy?: SyncPolicy): void
  commit(): Promise<void>
  setComposing(composing: boolean): void
  setFocused(focused: boolean): void
  onAdopt(listener: AdoptListener): () => void
}

export type EmitResult =
  ViewEventResult | { ok: false; reason: 'offline' | 'error'; message?: string }

const RESIZE_DEBOUNCE_MS = 150
const NOTICE_MS = 6_000
/** The most notices a window shows at once; the oldest go first (an unanswered Undo does not pile up forever). */
const MAX_NOTICES = 5

export class ViewCore {
  viewId = ''
  readonly store: ViewStore
  readonly shortcuts = new ShortcutRegistry()
  readonly bindings: Bindings
  readonly sources: Sources
  /** Mounted navigation stacks: where `nav.*` actions are routed. */
  readonly nav = new NavRegistry()
  private readonly clientState: ClientState
  private readonly link: ViewLink
  private readonly timers: Timers
  private effects: ViewEffects
  private readonly report: (error: unknown) => void
  private readonly stateRevs = new Map<string, number>()
  private openParams: ViewOpenParams | null = null

  /** The app this window belongs to (set by `open`): what scopes the app's chat threads. */
  get app(): string | undefined {
    return this.openParams?.app
  }
  private seq = 0
  private noticeId = 0
  /** The notices that expire: how long they get, and the running timer (none while held). */
  private readonly noticeLife = new Map<number, { ms: number; timer: unknown }>()
  private resyncing: Promise<void> | null = null
  private resizeTimer: unknown

  /** Format env with lazy getters: an expression only subscribes to `env.now` if it calls a function that reads it. */
  private readonly fmt: FormatEnv

  constructor(private o: ViewCoreOptions) {
    this.link = o.link
    this.timers = o.timers ?? realTimers
    this.effects = o.effects ?? {}
    this.report = o.onError ?? (() => undefined)
    this.store = (o.store ?? plainStore)()
    const store = this.store
    this.fmt = {
      get locale() {
        return store.env.locale
      },
      get timeZone() {
        return store.env.timeZone
      },
      get now() {
        return store.env.now
      }
    }
    this.store.link = this.link.online ? 'online' : 'offline'

    this.bindings = new Bindings({
      store,
      timers: this.timers,
      online: () => this.link.online,
      send: (target, value, baseRev) =>
        target.kind === 'state'
          ? this.link.request('state.set', {
              viewId: this.viewId,
              path: target.path,
              value,
              baseRev
            })
          : this.link.request('row.set', {
              viewId: this.viewId,
              source: target.source,
              key: target.key,
              field: target.field,
              value,
              baseRev
            }),
      read: (t) => this.readServer(t),
      adopt: (t, v, r) => this.adoptServer(t, v, r),
      notify: (kind, text, undo) =>
        this.notice(kind === 'conflict' ? 'conflict' : kind, text, NOTICE_MS, { undo }),
      announce: (text) => void (store.announcement = text),
      onError: this.report
    })
    this.sources = new Sources({
      store,
      timers: this.timers,
      online: () => this.link.online,
      request: (params) => this.link.request('source.set', { viewId: this.viewId, ...params }),
      evaluate: (expr) => this.evaluate(expr),
      onRow: (source, key, row, rev) => this.bindings.remoteRow(source, key, row, rev),
      onError: this.report
    })
    this.clientState = new ClientState({
      store,
      timers: this.timers,
      storage: o.storage,
      windowId: o.windowId,
      online: () => this.link.online,
      mirror: (set) => this.link.notify('client.state', { viewId: this.viewId, set }),
      onChange: () => this.sources.sync()
    })
  }

  // ── Lifecycle ──────────────────────────────────────────────────────────

  /** `view.open`: create the instance and install its first snapshot. */
  async open(params: ViewOpenParams): Promise<void> {
    this.openParams = params
    this.store.phase = 'opening'
    try {
      this.load(await this.link.request('view.open', params), false)
    } catch (error) {
      this.store.phase = 'failed'
      this.store.error = messageOf(error)
      throw error
    }
  }

  /** Install a full snapshot (open, resync, resume). Drafts and local client state survive it. */
  load(snap: ViewSnapshot, resumed: boolean): void {
    const { store } = this
    const { nodes, depth } = measure(snap.tree)
    if (nodes > MAX_NODES || depth > MAX_DEPTH)
      this.report(new Error(`tree exceeds the protocol limits (${nodes} nodes, depth ${depth})`))
    if (snap.viewId !== this.viewId) {
      const previous = this.viewId
      this.viewId = snap.viewId
      this.o.onViewId?.(snap.viewId, previous)
    }
    store.tree = snap.tree
    store.rev = snap.rev
    this.clientState.load(snap.clientDecls, snap.client)
    for (const [path, { v, r }] of Object.entries(snap.state ?? {}))
      this.bindings.remote({ kind: 'state', path }, v, r, 'snapshot')
    this.sources.load(snap.sources, snap.data, resumed)
    store.phase = 'ready'
    store.error = null
    this.sources.sync()
  }

  /** A full resync after a gap or a broken op. Concurrent triggers share one request. */
  resync(): Promise<void> {
    this.resyncing ??= (async () => {
      try {
        this.load(await this.link.request('view.sync', { viewId: this.viewId }), false)
      } catch (error) {
        if (!isConnectionError(error)) this.report(error)
      } finally {
        this.resyncing = null
      }
    })()
    return this.resyncing
  }

  /** The socket is back: resume this instance, then replay queued work. */
  async resume(): Promise<void> {
    const open = this.openParams
    if (!open) return
    const snap = await this.link.request('view.open', {
      ...open,
      resume: {
        viewId: this.viewId,
        rev: this.store.rev,
        client: this.clientState.snapshot(),
        drafts: this.bindings.pending()
      }
    })
    this.load(snap, true)
  }

  /** `view.closed {reason: "reloaded"}`: open the same window again (a fresh instance, drafts and client state kept). */
  private async reopen(): Promise<void> {
    try {
      await this.resume()
    } catch (error) {
      if (isConnectionError(error)) return // the reconnect path will resume it
      this.store.closed = messageOf(error)
      this.store.phase = 'closed'
      this.report(error)
    }
  }

  /** Replace the DOM effects (the host component installs them once it has a root element). */
  setEffects(effects: ViewEffects): void {
    this.effects = effects
  }

  /** Open a link the person chose (a Markdown link): the shell decides how, after its usual confirmation. */
  openUrl(url: string): void {
    this.effects.openUrl?.(url)
  }

  /** Say something to assistive technology (the window's polite live region). */
  announce(text: string): void {
    this.store.announcement = text
  }

  /** The link went up or down (the session calls this around the resume). */
  setLink(state: 'online' | 'offline'): void {
    this.store.link = state
    if (state === 'online') {
      this.clientState.flushMirror()
      void this.bindings.replay()
    }
  }

  /** `view.close`: flush drafts first, then the instance (and its source subscriptions) goes away. */
  async close(): Promise<void> {
    try {
      if (this.link.online) {
        await this.bindings.settle()
        this.clientState.flushMirror()
        await this.link.request('view.close', { viewId: this.viewId })
      }
    } catch (error) {
      if (!isConnectionError(error)) this.report(error)
    } finally {
      this.dispose()
      this.store.phase = 'closed'
    }
  }

  dispose(): void {
    this.bindings.dispose()
    this.sources.dispose()
    this.clientState.dispose()
    this.timers.clearTimeout(this.resizeTimer)
  }

  setVisible(visible: boolean): void {
    if (this.link.online && this.viewId)
      this.link.notify('view.visibility', { viewId: this.viewId, visible })
  }

  /** The window was resized: `Adaptive` / size classes switch locally; the host is told debounced. */
  resize(size: WindowSize): void {
    const sizeClass = size.w < 560 ? 'compact' : 'regular'
    this.store.env.sizeClass = sizeClass
    this.store.env.heightClass = size.h < 560 ? 'compact' : 'regular'
    this.timers.clearTimeout(this.resizeTimer)
    this.resizeTimer = this.timers.setTimeout(() => {
      if (this.link.online && this.viewId)
        this.link.notify('view.resize', { viewId: this.viewId, size, sizeClass })
    }, RESIZE_DEBOUNCE_MS)
  }

  // ── Host -> client ─────────────────────────────────────────────────────

  /** One host notification addressed to this view (the session routes by `viewId`). */
  notify<M extends keyof HostNotifications>(method: M, params: HostNotifications[M]): void {
    try {
      switch (method) {
        case 'view.tree': {
          const p = params as HostNotifications['view.tree']
          this.store.tree = p.tree
          this.store.rev = p.rev
          this.store.error = null
          return
        }
        case 'view.ops': {
          const p = params as HostNotifications['view.ops']
          if (!this.store.tree || p.base !== this.store.rev) return void this.resync()
          applyOps(this.store.tree, p.ops)
          this.store.rev = p.rev
          this.store.error = null
          return
        }
        case 'data.patch':
          return this.applyData(params as DataPatch)
        case 'client.set': {
          const p = params as HostNotifications['client.set']
          this.clientState.hostSet(p.path, p.value, p.force)
          return
        }
        case 'view.command':
          return this.command(params as HostNotifications['view.command'])
        case 'view.error':
          this.store.error = (params as HostNotifications['view.error']).message
          return
        case 'view.closed': {
          const reason = (params as HostNotifications['view.closed']).reason
          // A dev hot reload replaced the app code: the instance is gone but the app is not. Open it again,
          // carrying the draft and client state like a reconnect does.
          if (reason === 'reloaded' && this.openParams) return void this.reopen()
          this.store.closed = reason
          this.store.phase = 'closed'
          return
        }
      }
    } catch (error) {
      // A broken op means we diverged: never guess, ask for the full tree.
      if (error instanceof OpError) void this.resync()
      this.report(error)
    }
  }

  private applyData(p: DataPatch): void {
    if (p.declare) this.sources.declare(p.declare) // before the windows that use them
    for (const [path, { v, r }] of Object.entries(p.state ?? {}))
      this.bindings.remote({ kind: 'state', path }, v, r)
    for (const [id, patch] of Object.entries(p.sources ?? {})) this.sources.apply(id, patch)
    this.sources.sync()
  }

  private command(p: HostNotifications['view.command']): void {
    const c = p as { viewId: string } & ViewCommand
    switch (c.cmd) {
      case 'toast': {
        const args = c.args
        return this.notice(
          args.kind === 'error' ? 'error' : args.kind === 'success' ? 'success' : 'info',
          args.text,
          args.duration,
          { icon: args.icon, action: args.action }
        )
      }
      case 'announce':
        this.store.announcement = c.args.text
        return
      case 'focus':
        return this.withPath(c.args.key, (path) => this.effects.focus?.(path))
      case 'scrollTo':
        if (c.args.key !== undefined)
          return this.withPath(c.args.key, (path) => this.effects.scrollTo?.(path))
        return void (
          c.args.row !== undefined &&
          this.report(new Error('scrollTo {row} needs a windowed list (wave B)'))
        )
      case 'copy':
        if (this.effects.userActive && !this.effects.userActive())
          return this.report(
            new Error(
              'view.command copy ignored: the clipboard is only written right after a tap or key press'
            )
          )
        return this.effects.copy?.(c.args.text)
      case 'openUrl':
        return this.effects.openUrl?.(c.args.url)
      case 'haptic':
        return this.effects.haptic?.()
      case 'closeWindow':
        return this.effects.closeWindow?.()
    }
  }

  private withPath(key: string, run: (path: string) => void): void {
    const hit = this.store.tree ? findByKey(this.store.tree, key) : null
    if (hit) run(hit.path)
  }

  /**
   * Show a notice. One with an action or an Undo stays until the person answers or dismisses it: a timer would take it
   * away from anyone slower than six seconds, or a screen-reader user still moving to the button (WCAG 2.2.1). The
   * rest expire after `durationMs` (0 keeps them), and wait while the pointer or focus is on them (`holdNotice`).
   */
  notice(
    kind: NoticeKind,
    text: string,
    durationMs = NOTICE_MS,
    extra: { icon?: string; action?: Notice['action']; undo?: Notice['undo'] } = {}
  ): void {
    const id = ++this.noticeId
    const notice: Notice = { id, kind, text }
    if (extra.icon) notice.icon = extra.icon
    if (extra.action) notice.action = extra.action
    if (extra.undo) notice.undo = extra.undo
    this.store.notices.push(notice)
    while (this.store.notices.length > MAX_NOTICES) {
      const plain = this.store.notices.find((n) => !n.action && !n.undo)
      this.dismissNotice((plain ?? this.store.notices[0]).id)
    }
    if (durationMs > 0 && !notice.action && !notice.undo) this.expireAfter(id, durationMs)
  }

  private expireAfter(id: number, ms: number): void {
    this.noticeLife.set(id, { ms, timer: this.timers.setTimeout(() => this.dismissNotice(id), ms) })
  }

  /** The pointer or focus is on a notice: it stays while it is read. */
  holdNotice(id: number): void {
    const life = this.noticeLife.get(id)
    if (!life?.timer) return
    this.timers.clearTimeout(life.timer)
    life.timer = undefined
  }

  /** The pointer and focus left the notice: it gets its full time again. */
  releaseNotice(id: number): void {
    const life = this.noticeLife.get(id)
    if (life && life.timer === undefined && this.store.notices.some((n) => n.id === id))
      this.expireAfter(id, life.ms)
  }

  /**
   * The action button of a toast was tapped: dismiss it and report the tap as `view.event` on `$toast/<id>`, where
   * the host keeps the app's `onTap` closure (it runs once).
   */
  toastAction(id: number): void {
    const action = this.store.notices.find((n) => n.id === id)?.action
    this.dismissNotice(id)
    if (action) void this.emit(`$toast/${action.id}`, 'tap')
  }

  /**
   * The notice's "Undo": ask the host to restore the value a write replaced (`row.restore`), adopt the ack like any
   * other write's, and say so. A restore the host refuses leaves the value alone and shows the error.
   */
  async undo(id: number): Promise<void> {
    const notice = this.store.notices.find((n) => n.id === id)
    const undo = notice?.undo
    this.dismissNotice(id)
    if (!undo) return
    try {
      const { source, key, field, revisionId } = undo
      const ack = await this.link.request('row.restore', {
        viewId: this.viewId,
        source,
        key,
        revisionId
      })
      this.adoptServer({ kind: 'row', source, key, field }, ack.value, ack.rev)
      this.store.announcement = 'Restored the earlier value'
    } catch (error) {
      if (isConnectionError(error))
        this.notice('error', 'You are offline. The edit was not undone.')
      else
        this.notice('error', error instanceof Error ? error.message : 'Could not undo that edit.')
    }
  }

  dismissNotice(id: number): void {
    this.holdNotice(id)
    this.noticeLife.delete(id)
    const i = this.store.notices.findIndex((n) => n.id === id)
    if (i >= 0) this.store.notices.splice(i, 1)
  }

  // ── Reading: refs and expressions ───────────────────────────

  /** The longest `state.<path>` the store knows (state paths are granular, dotted names). */
  private statePath(rest: string[]): { path: string; tail: string[] } {
    for (let i = rest.length; i > 0; i--) {
      const path = rest.slice(0, i).join('.')
      if (bindingKey({ kind: 'state', path }) in this.store.drafts || path in this.store.state)
        return { path, tail: rest.slice(i) }
    }
    return { path: rest.join('.'), tail: [] }
  }

  resolve(ref: string, scope: Scope = {}): unknown {
    const { store } = this
    const [ns, ...rest] = ref.split('.')
    switch (ns) {
      case 'state': {
        const { path, tail } = this.statePath(rest)
        const key = bindingKey({ kind: 'state', path })
        return dig(key in store.drafts ? store.drafts[key] : store.state[path], tail)
      }
      case 'client':
        return dig(store.client[rest[0]], rest.slice(1))
      case 'row': {
        if (!scope.row) return undefined
        const draft =
          scope.source !== undefined && scope.rowKey !== undefined
            ? this.draftOf({ kind: 'row', source: scope.source, key: scope.rowKey, field: rest[0] })
            : undefined
        return dig(draft !== undefined ? { [rest[0]]: draft } : scope.row, rest)
      }
      case 'src':
        return this.resolveSource(rest)
      case 'env':
        return dig(store.env, rest)
      case 'route':
        return dig(scope.route, rest)
      default:
        return undefined
    }
  }

  private resolveSource([id, field, ...tail]: string[]): unknown {
    const data = this.store.sources[id]
    if (!data) return undefined
    switch (data.kind) {
      case 'row': {
        if (field === undefined) return data.row
        const key = data.row?.[this.sources.keyField(id)]
        const draft =
          key === undefined || key === null
            ? undefined
            : this.draftOf({ kind: 'row', source: id, key: key as RowKey, field })
        return dig(draft !== undefined ? draft : data.row?.[field], tail)
      }
      case 'rows':
        if (field === 'total') return data.total
        if (field === 'count') return data.keys.length
        if (field === 'hasMore') return data.from + data.keys.length < data.total
        return undefined
      case 'value':
        return dig(data.value, field === undefined ? [] : [field, ...tail])
    }
  }

  private draftOf(t: BindingTarget): Json | undefined {
    return this.store.drafts[bindingKey(t)]
  }

  evaluate(expr: Expr | undefined, scope: Scope = {}): unknown {
    return evaluate(expr, (r) => this.resolve(r, scope), this.fmt)
  }

  // ── Writing: bindings ───────────────────────────

  /** Which server target a ref writes, or null when the namespace is read-only. */
  target(ref: string, scope: Scope = {}): BindingTarget | null {
    const [ns, ...rest] = ref.split('.')
    if (ns === 'state' && rest.length) return { kind: 'state', path: this.statePath(rest).path }
    if (ns === 'row' && rest.length && scope.source !== undefined && scope.rowKey !== undefined)
      return { kind: 'row', source: scope.source, key: scope.rowKey, field: rest[0] }
    if (ns === 'src' && rest.length >= 2) {
      const [id, field] = rest
      const data = this.store.sources[id]
      const key = data?.kind === 'row' ? data.row?.[this.sources.keyField(id)] : undefined
      const writable = this.sources.writable(id)
      if (key !== undefined && key !== null && (!writable || writable.includes(field)))
        return { kind: 'row', source: id, key: key as RowKey, field }
    }
    return null
  }

  /** The two-way handle for a `Binding` prop (null when the prop is a literal, an expression or read-only). */
  bindingOf(expr: Expr | undefined, scope: Scope = {}): Binding | null {
    if (!isRef(expr)) return null
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this
    const ref = expr.$ref
    const [ns, ...rest] = ref.split('.')
    if (ns === 'client') {
      const name = rest[0]
      return {
        ref,
        tier: 'client',
        get value() {
          return self.resolve(ref, scope)
        },
        dirty: false,
        error: undefined,
        set: (value) => this.clientState.set(rest.join('.'), value),
        commit: () => Promise.resolve(),
        setComposing: () => undefined,
        setFocused: (focused) => this.clientState.setFocused(name, focused),
        onAdopt: () => () => undefined
      }
    }
    const target = this.target(ref, scope)
    if (!target) return null
    const key = bindingKey(target)
    return {
      ref,
      tier: 'server',
      get value() {
        return self.resolve(ref, scope)
      },
      get dirty() {
        return self.store.dirty[key] === true
      },
      get error() {
        return self.store.fieldErrors[key]
      },
      set: (value, policy) =>
        this.bindings.edit(
          target,
          value,
          policy ?? (typeof value === 'string' ? TEXT_SYNC : SCALAR_SYNC)
        ),
      commit: () => this.bindings.commit(target),
      setComposing: (composing) => this.bindings.setComposing(target, composing),
      setFocused: () => undefined,
      onAdopt: (listener) => this.bindings.onAdopt(target, listener)
    }
  }

  /** Programmatic write by ref (client actions, tests). Throws for a read-only namespace. */
  write(ref: string, value: Json, scope: Scope = {}, policy?: SyncPolicy): void {
    const binding = this.bindingOf({ $ref: ref }, scope)
    if (!binding) throw new Error(`ref "${ref}" is not writable here`)
    binding.set(value, policy)
  }

  /** `sync` prop -> policy. */
  syncPolicy(spec: unknown, fallback: SyncPolicy = TEXT_SYNC): SyncPolicy {
    return parseSync(spec, fallback)
  }

  private readServer(t: BindingTarget): { value: Json; rev: number } | undefined {
    if (t.kind === 'state')
      return t.path in this.store.state
        ? { value: this.store.state[t.path], rev: this.stateRevs.get(t.path) ?? 0 }
        : undefined
    const data = this.store.sources[t.source]
    if (data?.kind === 'row')
      return data.row ? { value: data.row[t.field] ?? null, rev: data.rev } : undefined
    if (data?.kind === 'rows') {
      const row = data.rows[String(t.key)]
      return row
        ? { value: row[t.field] ?? null, rev: typeof row.rev === 'number' ? row.rev : 0 }
        : undefined
    }
    return undefined
  }

  private adoptServer(t: BindingTarget, value: Json, rev: number): void {
    if (t.kind === 'state') {
      this.store.state[t.path] = value
      this.stateRevs.set(t.path, rev)
      return
    }
    const data = this.store.sources[t.source]
    if (data?.kind === 'row' && data.row) {
      data.row[t.field] = value
      data.rev = rev
    } else if (data?.kind === 'rows') {
      const row = data.rows[String(t.key)]
      if (row) {
        row[t.field] = value
        if ('rev' in row) row.rev = rev
      }
    }
  }

  // ── Events and actions ─────────────────────────────────

  /**
   * A node reported an event: run its handler (client actions first, `emit` entries towards the app). The promise
   * settles when the app has answered every `emit` (a list's "pull to refresh" spinner runs until then).
   */
  fire(node: Node, path: string, event: string, scope: Scope = {}, payload?: Json): Promise<void> {
    const handler: Handler | undefined = node.on?.[event]
    if (!handler) return Promise.resolve()
    if (handler === true) return this.emit(path, event, scope.rowKey, payload).then(() => undefined)
    const undo: { ref: string; before: unknown; after: unknown; scope: Scope }[] = []
    const sent: Promise<EmitResult>[] = []
    for (const a of handler) this.run(a, { path, event, scope, undo, sent })
    if (undo.length && sent.length) void this.restoreOnError(sent, undo)
    return Promise.all(sent).then(() => undefined)
  }

  private run(a: Action, ctx: ActionCtx): void {
    const { scope } = ctx
    switch (a.do) {
      case 'set': {
        const value = this.evaluate(a.value, scope) as Json
        return this.optimistic(a.ref, value, ctx)
      }
      case 'toggle':
        return this.optimistic(a.ref, !this.resolve(a.ref, scope), ctx)
      case 'emit': {
        const payload = a.payload
          ? (Object.fromEntries(
              Object.entries(a.payload).map(([k, e]) => [
                k,
                (this.evaluate(e, scope) ?? null) as Json
              ])
            ) as Json)
          : undefined
        ctx.sent.push(this.emit(ctx.path, a.event ?? ctx.event, scope.rowKey, payload))
        return
      }
      case 'seq':
        for (const inner of a.actions) this.run(inner, ctx)
        return
      case 'focus':
        return this.withPath(a.key, (path) => this.effects.focus?.(path))
      case 'scrollTo':
        return this.withPath(a.key, (path) => this.effects.scrollTo?.(path))
      case 'copy':
        return this.effects.copy?.(String(this.evaluate(a.text, scope) ?? ''))
      case 'nav.push':
        return this.navigate(ctx.path, a.route, scope, a.stack)
      case 'nav.pop':
      case 'nav.popToRoot': {
        const stack = this.nav.resolve(ctx.path)
        if (!stack)
          return this.report(new Error(`${a.do}: no NavigationStack contains ${ctx.path}`))
        return a.do === 'nav.pop' ? stack.pop() : stack.popToRoot()
      }
    }
  }

  /**
   * Push a route on the stack an action or a `NavigationLink` addresses: the innermost stack that
   * contains the node at `from`, or the one named `stack`. Param values are expressions, read in `scope` now.
   */
  navigate(from: string, route: Record<string, Expr>, scope: Scope = {}, stack?: string): void {
    const target = this.nav.resolve(from, stack)
    if (!target) return this.report(new Error(`nav.push: no NavigationStack contains ${from}`))
    const value = Object.fromEntries(
      Object.entries(route).map(([k, e]) => [k, (this.evaluate(e, scope) ?? null) as Json])
    )
    if (typeof value.name !== 'string' || !value.name)
      return this.report(new Error('nav.push: the route needs a name'))
    target.push({ ...value, name: value.name })
  }

  /** Apply a `set`/`toggle` at once and remember the before value so a failed event can restore it. */
  private optimistic(ref: string, value: Json, ctx: ActionCtx): void {
    const before = this.resolve(ref, ctx.scope)
    this.write(ref, value, ctx.scope)
    ctx.undo.push({ ref, before, after: value, scope: ctx.scope })
  }

  /** The event's response was an error: restore the before values, unless a newer value replaced them already. */
  private async restoreOnError(
    sent: Promise<EmitResult>[],
    undo: { ref: string; before: unknown; after: unknown; scope: Scope }[]
  ): Promise<void> {
    const results = await Promise.all(sent)
    if (!results.some((r) => !r.ok && (r.reason === 'error' || r.reason === 'offline'))) return
    for (const u of undo.reverse()) {
      if (JSON.stringify(this.resolve(u.ref, u.scope)) === JSON.stringify(u.after))
        this.write(u.ref, u.before as Json, u.scope)
    }
  }

  /**
   * Send an event addressed by key path. Dirty drafts flush first and are acked, so the app never sees "Save"
   * before the text typed before pressing it. Events are never queued offline (a tap is not idempotent).
   */
  async emit(path: string, event: string, rowKey?: RowKey, payload?: Json): Promise<EmitResult> {
    if (!this.link.online) {
      this.notice('error', 'You are offline. That action was not sent.')
      return { ok: false, reason: 'offline' }
    }
    try {
      await this.bindings.settle()
      this.clientState.flushMirror()
      return await this.link.request('view.event', {
        viewId: this.viewId,
        rev: this.store.rev,
        seq: ++this.seq,
        path,
        rowKey,
        event,
        payload
      })
    } catch (error) {
      if (isConnectionError(error)) {
        this.notice('error', 'The connection dropped. That action was not sent.')
        return { ok: false, reason: 'offline' }
      }
      const message =
        error instanceof RpcError && error.code === AppErrorCode.AppError
          ? error.message
          : 'Something went wrong.'
      this.notice('error', message)
      this.report(error)
      return { ok: false, reason: 'error', message }
    }
  }
}

interface ActionCtx {
  path: string
  event: string
  scope: Scope
  undo: { ref: string; before: unknown; after: unknown; scope: Scope }[]
  sent: Promise<EmitResult>[]
}
