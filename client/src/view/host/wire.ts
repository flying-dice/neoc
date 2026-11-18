/**
 * An in-memory "WebSocket" between the client's real `createWebSocketTransport` and a fake host. Frames are real
 * JSON strings, delivery is FIFO with a configurable one-way latency, and the link can be dropped and restored,
 * so the production reconnect/backoff code is what runs in tests and stories. Test support.
 */
import { realTimers, type Timers } from '../core/timers'
import { createWebSocketTransport, type Backoff, type SocketLike } from '../rpc/websocket'
import type { Transport } from '../rpc/transport'

export interface WireMessage {
  dir: 'c2h' | 'h2c'
  text: string
  at: number
}

/** The host's end of one connection. */
export interface ServerConnection {
  send(message: string): void
  onMessage(listener: (message: string) => void): void
  onClose(listener: () => void): void
  close(): void
}

export interface MemoryLinkOptions {
  latencyMs?: number
  timers?: Timers
  backoff?: Backoff
  random?: () => number
  onMessage?: (m: WireMessage) => void
}

export interface MemoryLink {
  /** The client end: hand it to a `Session`. */
  transport: Transport
  /** The host end: called for every accepted connection. */
  onConnection(handler: (connection: ServerConnection) => void): void
  setLatency(ms: number): void
  /** Network failure: close the current connection from the host side; the client reconnects with backoff. */
  drop(): void
  /** While unreachable, connection attempts fail (the host is down or the network is out). */
  setReachable(reachable: boolean): void
  readonly log: WireMessage[]
  readonly connections: number
}

export function createMemoryLink(options: MemoryLinkOptions = {}): MemoryLink {
  const timers = options.timers ?? realTimers
  const t0 = timers.now()
  let latency = options.latencyMs ?? 0
  let reachable = true
  let accept: ((c: ServerConnection) => void) | undefined
  let current: FakeSocket | null = null
  let connections = 0
  const log: WireMessage[] = []

  const deliver = (dir: WireMessage['dir'], text: string, to: () => void): void => {
    const m = { dir, text, at: timers.now() - t0 }
    log.push(m)
    options.onMessage?.(m)
    if (latency > 0) timers.setTimeout(to, latency)
    else queueMicrotask(to)
  }

  class FakeSocket implements SocketLike {
    readyState = 0
    onopen: ((ev: unknown) => void) | null = null
    onclose: ((ev: unknown) => void) | null = null
    onerror: ((ev: unknown) => void) | null = null
    onmessage: ((ev: { data: unknown }) => void) | null = null
    private serverMessage: ((m: string) => void) | undefined
    private serverClose: (() => void) | undefined

    constructor() {
      queueMicrotask(() => {
        if (!reachable || !accept) return this.finish()
        this.readyState = 1
        // eslint-disable-next-line @typescript-eslint/no-this-alias -- the link tracks its live socket
        current = this
        connections++
        accept({
          send: (m) =>
            deliver('h2c', m, () => this.readyState === 1 && this.onmessage?.({ data: m })),
          onMessage: (l) => void (this.serverMessage = l),
          onClose: (l) => void (this.serverClose = l),
          close: () => this.finish()
        })
        this.onopen?.({})
      })
    }

    send(data: string): void {
      deliver('c2h', data, () => this.readyState === 1 && this.serverMessage?.(data))
    }
    /** A client-initiated close lets frames already sent (a `session.bye`) arrive first, like a real socket. */
    close(): void {
      queueMicrotask(() => this.finish())
    }
    /** Close from either side exactly once. */
    finish(): void {
      if (this.readyState === 3) return
      this.readyState = 3
      if (current === this) current = null
      this.serverClose?.()
      queueMicrotask(() => this.onclose?.({}))
    }
  }

  const transport = createWebSocketTransport({
    url: 'memory://rpc',
    createSocket: () => new FakeSocket(),
    backoff: options.backoff ?? { baseMs: 500, maxMs: 4000, factor: 2, jitter: 0 },
    random: options.random ?? (() => 0),
    setTimer: (fn, ms) => timers.setTimeout(fn, ms) as ReturnType<typeof setTimeout>,
    clearTimer: (t) => timers.clearTimeout(t)
  })

  return {
    transport,
    onConnection: (h) => void (accept = h),
    setLatency: (ms) => void (latency = ms),
    drop: () => current?.finish(),
    setReachable: (r) => void (reachable = r),
    log,
    get connections() {
      return connections
    }
  }
}
