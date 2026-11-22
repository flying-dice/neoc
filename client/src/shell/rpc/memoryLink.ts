/**
 * An in-memory "WebSocket" between the client's real `createWebSocketTransport` and a fake host. Frames are real
 * JSON strings, delivery is FIFO, and the link can be dropped and restored, so the production reconnect/backoff
 * code is what runs in tests. Test support.
 */
import type { Timers } from './fakeTimers'
import { createWebSocketTransport, type SocketLike } from './websocket'
import type { Transport } from './transport'

/** The host's end of one connection. */
interface ServerConnection {
  send(message: string): void
  onMessage(listener: (message: string) => void): void
  onClose(listener: () => void): void
  close(): void
}

interface MemoryLinkOptions {
  timers: Timers
}

interface MemoryLink {
  /** The client end: hand it to a `Session`. */
  transport: Transport
  /** The host end: called for every accepted connection. */
  onConnection(handler: (connection: ServerConnection) => void): void
  /** Network failure: close the current connection from the host side; the client reconnects with backoff. */
  drop(): void
  /** While unreachable, connection attempts fail (the host is down or the network is out). */
  setReachable(reachable: boolean): void
  readonly connections: number
}

export function createMemoryLink(options: MemoryLinkOptions): MemoryLink {
  const { timers } = options
  let reachable = true
  let accept: ((c: ServerConnection) => void) | undefined
  let current: FakeSocket | null = null
  let connections = 0

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
          send: (m) => queueMicrotask(() => this.readyState === 1 && this.onmessage?.({ data: m })),
          onMessage: (l) => void (this.serverMessage = l),
          onClose: (l) => void (this.serverClose = l),
          close: () => this.finish()
        })
        this.onopen?.({})
      })
    }

    send(data: string): void {
      queueMicrotask(() => this.readyState === 1 && this.serverMessage?.(data))
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
    backoff: { baseMs: 500, maxMs: 4000, factor: 2, jitter: 0 },
    random: () => 0,
    setTimer: (fn, ms) => timers.setTimeout(fn, ms) as ReturnType<typeof setTimeout>,
    clearTimer: (t) => timers.clearTimeout(t)
  })

  return {
    transport,
    onConnection: (h) => void (accept = h),
    drop: () => current?.finish(),
    setReachable: (r) => void (reachable = r),
    get connections() {
      return connections
    }
  }
}
