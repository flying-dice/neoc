/**
 * The session: one connection to the host, shared by every window's view instance (spec 2.5, 2.6, 3.5).
 *
 *  - owns the transport and one JSON-RPC peer per connection;
 *  - handshakes with `session.hello` (version negotiation, formatting env);
 *  - routes host notifications to views by `viewId` (`theme.set` is session-wide);
 *  - on disconnect every view stays mounted and goes offline (dimmed, drafts kept, writes queued);
 *  - on reconnect: `session.hello {resume}`, then each view resumes (`view.open {resume}`), reconciles the snapshot
 *    through the same rules as live patches, and replays its queued drafts.
 */
import {
  KIT,
  PROTO,
  type ClientNotifications,
  type HostNotifications,
  type RequestMethod,
  type RequestMethods,
  type SessionHelloResult,
  type ThemeSet,
  type ViewOpenParams
} from '../protocol'
import { createRpcPeer, RpcError, RpcErrorCode, type Params, type RpcPeer } from '../rpc/peer'
import type { Transport, TransportStatus } from '../rpc/transport'
import type { KeyValueStorage } from './clientState'
import { plainStore, type StoreFactory } from './store'
import { realTimers, type Timers } from './timers'
import { ViewCore, type ViewEffects, type ViewLink } from './view'

/** The oldest `proto` this client can speak. The host negotiates down to `min(proto)`; below this it is refused. */
export const MIN_PROTO = 1

export type SessionState = 'connecting' | 'online' | 'offline' | 'incompatible'

export interface SessionOptions {
  transport: Transport
  locale?: string
  timeZone?: string
  features?: string[]
  createStore?: StoreFactory
  storage?: KeyValueStorage
  timers?: Timers
  onTheme?(theme: ThemeSet): void
  onError?(error: unknown): void
  requestTimeoutMs?: number
}

const HOST_NOTIFICATIONS: readonly string[] = ['view.tree', 'view.ops', 'data.patch', 'client.set', 'view.command', 'view.error', 'view.closed']

export class Session implements ViewLink {
  state: SessionState = 'connecting'
  host: SessionHelloResult | null = null
  private sessionId: string | null = null
  private peer: RpcPeer | null = null
  private ready = false
  private everOnline = false
  private readonly views = new Map<string, ViewCore>()
  private readonly all = new Set<ViewCore>()
  private readonly listeners = new Set<(state: SessionState) => void>()
  private readonly unsub: (() => void)[] = []
  private readonly timers: Timers

  constructor(private o: SessionOptions) {
    this.timers = o.timers ?? realTimers
  }

  get online(): boolean {
    return this.ready
  }

  /** Subscribe to connection state changes (a global "offline" indicator). */
  onState(listener: (state: SessionState) => void): () => void {
    this.listeners.add(listener)
    return () => void this.listeners.delete(listener)
  }

  private setState(state: SessionState): void {
    if (this.state === state) return
    this.state = state
    for (const l of [...this.listeners]) l(state)
  }

  /** Connect (and reconnect, with the transport's backoff). */
  start(): void {
    const { transport } = this.o
    this.unsub.push(
      transport.onMessage((m) => this.peer?.receive(m)),
      transport.onStatus((s) => this.onStatus(s))
    )
    transport.connect()
    if (transport.status === 'open') this.onStatus('open')
  }

  stop(): void {
    this.peer?.notify('session.bye', {})
    this.dropConnection()
    this.o.transport.close()
    for (const u of this.unsub.splice(0)) u()
  }

  retryNow(): void {
    this.o.transport.retryNow()
  }

  // ── Connection ─────────────────────────────────────────────────────────

  private onStatus(status: TransportStatus): void {
    if (status === 'open') return void this.connected()
    this.dropConnection()
    if (this.state !== 'incompatible') this.setState(this.everOnline ? 'offline' : 'connecting')
  }

  private dropConnection(): void {
    this.ready = false
    this.peer?.close()
    this.peer = null
    for (const view of this.all) view.setLink('offline')
  }

  private async connected(): Promise<void> {
    const peer = createRpcPeer({
      send: (m) => this.o.transport.send(m),
      onNotification: (method, params) => this.route(method, params),
      onError: this.o.onError,
      timeoutMs: this.o.requestTimeoutMs
    })
    this.peer = peer
    try {
      const resume = this.sessionId ? { sessionId: this.sessionId, views: [...this.views].map(([viewId, v]) => ({ viewId, rev: v.store.rev })) } : undefined
      const hello = await peer.request<SessionHelloResult>('session.hello', {
        proto: PROTO,
        kit: KIT,
        locale: this.o.locale ?? navigator.language,
        timeZone: this.o.timeZone ?? Intl.DateTimeFormat().resolvedOptions().timeZone,
        features: this.o.features ?? [],
        ...(resume ? { resume } : {})
      })
      if (this.peer !== peer) return // the socket dropped during the handshake
      if (hello.proto < MIN_PROTO) {
        this.setState('incompatible')
        return
      }
      this.host = hello
      this.sessionId = hello.sessionId
      this.ready = true
      this.everOnline = true
      this.setState('online')
      await Promise.all([...this.all].map((view) => this.resumeView(view, peer)))
    } catch (error) {
      if (this.peer === peer && error instanceof RpcError && error.code !== RpcErrorCode.ConnectionClosed && error.code !== RpcErrorCode.Timeout) {
        this.o.onError?.(error)
        this.setState('incompatible') // the host refused the handshake: "reload the app"
      }
    }
  }

  private async resumeView(view: ViewCore, peer: RpcPeer): Promise<void> {
    try {
      await view.resume()
      if (this.peer === peer) view.setLink('online')
    } catch (error) {
      if (this.peer === peer) this.o.onError?.(error)
    }
  }

  private route(method: string, params: Params): void {
    if (method === 'theme.set') return this.o.onTheme?.(params as unknown as ThemeSet)
    if (!HOST_NOTIFICATIONS.includes(method)) return
    const viewId = params.viewId
    if (typeof viewId !== 'string') return
    this.views.get(viewId)?.notify(method as keyof HostNotifications, params as never)
  }

  // ── ViewLink ───────────────────────────────────────────────────────────

  request<M extends RequestMethod>(method: M, params: RequestMethods[M]['params']): Promise<RequestMethods[M]['result']> {
    const peer = this.peer
    if (!peer || (!this.ready && method !== 'session.hello' && method !== 'view.open'))
      return Promise.reject(new RpcError(RpcErrorCode.ConnectionClosed, 'Not connected'))
    return peer.request(method, params as unknown as Params)
  }

  notify<M extends keyof ClientNotifications>(method: M, params: ClientNotifications[M]): void {
    if (this.ready) this.peer?.notify(method, params as unknown as Params)
  }

  // ── Views ──────────────────────────────────────────────────────────────

  /** Open a view instance for a window. Waits for the connection if it is not up yet. */
  async openView(params: Omit<ViewOpenParams, 'resume'>, opts: { effects?: ViewEffects } = {}): Promise<ViewCore> {
    const view = new ViewCore({
      link: this,
      windowId: params.windowId,
      store: this.o.createStore ?? plainStore,
      storage: this.o.storage,
      timers: this.timers,
      effects: opts.effects,
      onError: this.o.onError,
      onViewId: (id, previous) => {
        if (previous) this.views.delete(previous)
        this.views.set(id, view)
      }
    })
    this.all.add(view)
    if (!this.ready) await this.whenOnline()
    try {
      await view.open(params)
    } catch (error) {
      this.all.delete(view)
      throw error
    }
    return view
  }

  async closeView(view: ViewCore): Promise<void> {
    await view.close()
    this.views.delete(view.viewId)
    this.all.delete(view)
  }

  private whenOnline(): Promise<void> {
    if (this.ready) return Promise.resolve()
    return new Promise((resolve) => {
      const off = this.onState((s) => {
        if (s !== 'online') return
        off()
        resolve()
      })
    })
  }
}
