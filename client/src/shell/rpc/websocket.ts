/**
 * The production transport: one WebSocket to `/rpc`, reconnecting with exponential backoff and jitter.
 * This is the only file in the client that opens a raw WebSocket (allow-listed in `lint:boundaries`); every
 * other module talks to the `Transport` interface. The session above it says hello again on each (re)connect.
 */
import { Emitter, type Transport, type TransportStatus } from './transport'

export interface Backoff {
  baseMs: number
  maxMs: number
  factor: number
  /** 0..1: the delay is spread over [delay * (1 - jitter), delay]. */
  jitter: number
}

const DEFAULT_BACKOFF: Backoff = { baseMs: 500, maxMs: 15_000, factor: 2, jitter: 0.25 }

/** Delay before reconnect attempt number `attempt` (0 = the first retry). */
export function backoffDelay(
  attempt: number,
  backoff: Backoff = DEFAULT_BACKOFF,
  random: () => number = Math.random
): number {
  const raw = Math.min(backoff.maxMs, backoff.baseMs * backoff.factor ** attempt)
  return Math.round(raw * (1 - backoff.jitter * random()))
}

/** The slice of the browser WebSocket this transport uses (so tests can inject a fake). */
export interface SocketLike {
  readyState: number
  send(data: string): void
  close(): void
  onopen: ((ev: unknown) => void) | null
  onclose: ((ev: unknown) => void) | null
  onerror: ((ev: unknown) => void) | null
  onmessage: ((ev: { data: unknown }) => void) | null
}

const OPEN = 1

export interface WebSocketTransportOptions {
  /** Full ws(s):// URL of the host's /rpc endpoint. */
  url: string
  backoff?: Backoff
  createSocket?: (url: string) => SocketLike
  random?: () => number
  setTimer?: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>
  clearTimer?: (t: ReturnType<typeof setTimeout>) => void
}

/** `ws://host/rpc` for the page the client was served from. */
export function defaultRpcUrl(location: Pick<Location, 'protocol' | 'host'>): string {
  return `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/rpc`
}

export function createWebSocketTransport(options: WebSocketTransportOptions): Transport {
  const backoff = options.backoff ?? DEFAULT_BACKOFF
  const createSocket =
    options.createSocket ?? ((url: string) => new WebSocket(url) as unknown as SocketLike)
  const random = options.random ?? Math.random
  const setTimer = options.setTimer ?? ((fn, ms) => setTimeout(fn, ms))
  const clearTimer = options.clearTimer ?? ((t) => clearTimeout(t))
  const messages = new Emitter<string>()
  const statuses = new Emitter<TransportStatus>()
  let socket: SocketLike | null = null
  let status: TransportStatus = 'closed'
  let attempt = 0
  let retry: ReturnType<typeof setTimeout> | undefined
  let stopped = true

  const setStatus = (next: TransportStatus): void => {
    if (status === next) return
    status = next
    statuses.emit(next)
  }

  function open(): void {
    cancelRetry()
    setStatus('connecting')
    let ws: SocketLike
    try {
      ws = createSocket(options.url)
    } catch {
      scheduleRetry()
      return
    }
    socket = ws
    ws.onopen = () => {
      if (socket !== ws) return
      attempt = 0
      setStatus('open')
    }
    ws.onmessage = (ev) => {
      if (socket === ws && typeof ev.data === 'string') messages.emit(ev.data)
    }
    ws.onerror = () => undefined // `close` always follows and carries the recovery
    ws.onclose = () => {
      if (socket !== ws) return
      socket = null
      setStatus('closed')
      if (!stopped) scheduleRetry()
    }
  }

  function cancelRetry(): void {
    if (retry !== undefined) clearTimer(retry)
    retry = undefined
  }

  function scheduleRetry(): void {
    setStatus('closed')
    retry = setTimer(open, backoffDelay(attempt++, backoff, random))
  }

  return {
    get status() {
      return status
    },
    send(message) {
      if (!socket || socket.readyState !== OPEN) throw new Error('Not connected')
      socket.send(message)
    },
    connect() {
      stopped = false
      if (!socket && retry === undefined) open()
    },
    close() {
      stopped = true
      cancelRetry()
      const ws = socket
      socket = null
      ws?.close()
      setStatus('closed')
    },
    retryNow() {
      if (stopped || socket) return
      attempt = 0
      open()
    },
    onMessage: (l) => messages.on(l),
    onStatus: (l) => statuses.on(l)
  }
}
