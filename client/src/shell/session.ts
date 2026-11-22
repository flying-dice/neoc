/**
 * The session: the shell's one connection to the host.
 *
 *  - owns the transport and one JSON-RPC peer per connection;
 *  - handshakes with `session.hello` (version negotiation, formatting env);
 *  - reports where the connection stands (`connecting`, `online`, `offline`, `incompatible`), which the shell shows;
 *  - on reconnect says hello again with the session it had (`session.hello {resume}`), so the host can tell a
 *    client coming back from a new one.
 *
 * It asks the host nothing else: the shell's own calls (`app.*`, `host.*`) and the host's notifications travel on
 * the same transport through `channel.ts`.
 */
import { KIT, PROTO, type SessionHello, type SessionWelcome } from './protocol'
import { createRpcPeer, RpcError, RpcErrorCode, type RpcPeer } from './rpc/peer'
import type { Transport, TransportStatus } from './rpc/transport'

/** The oldest `proto` this client can speak. The host negotiates down to `min(proto)`; below this it is refused. */
const MIN_PROTO = 1

export type SessionState = 'connecting' | 'online' | 'offline' | 'incompatible'

export interface SessionOptions {
  transport: Transport
  locale?: string
  timeZone?: string
  features?: string[]
  onError?(error: unknown): void
}

export class Session {
  state: SessionState = 'connecting'
  host: SessionWelcome | null = null
  private sessionId: string | null = null
  private peer: RpcPeer | null = null
  private ready = false
  private everOnline = false
  private readonly listeners = new Set<(state: SessionState) => void>()
  private readonly unsub: (() => void)[] = []

  constructor(private o: SessionOptions) {}

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

  private onStatus(status: TransportStatus): void {
    if (status === 'open') return void this.connected()
    this.dropConnection()
    if (this.state !== 'incompatible') this.setState(this.everOnline ? 'offline' : 'connecting')
  }

  private dropConnection(): void {
    this.ready = false
    this.peer?.close()
    this.peer = null
  }

  private async connected(): Promise<void> {
    const peer = createRpcPeer({
      send: (m) => this.o.transport.send(m),
      onError: this.o.onError
    })
    this.peer = peer
    try {
      const hello: SessionHello = {
        proto: PROTO,
        kit: KIT,
        locale: this.o.locale ?? navigator.language,
        timeZone: this.o.timeZone ?? Intl.DateTimeFormat().resolvedOptions().timeZone,
        features: this.o.features ?? [],
        ...(this.sessionId ? { resume: { sessionId: this.sessionId, views: [] } } : {})
      }
      const welcome = await peer.request<SessionWelcome>('session.hello', { ...hello })
      if (this.peer !== peer) return // the socket dropped during the handshake
      if (welcome.proto < MIN_PROTO) {
        this.setState('incompatible')
        return
      }
      this.host = welcome
      this.sessionId = welcome.sessionId
      this.ready = true
      this.everOnline = true
      this.setState('online')
    } catch (error) {
      if (
        this.peer === peer &&
        error instanceof RpcError &&
        error.code !== RpcErrorCode.ConnectionClosed &&
        error.code !== RpcErrorCode.Timeout
      ) {
        this.o.onError?.(error)
        this.setState('incompatible') // the host refused the handshake: "reload the app"
      }
    }
  }
}
