/**
 * The shell's side channel on the session's one connection.
 *
 * The `Session` makes the handshake (`session.hello`) and nothing else. The shell also needs the host's own
 * methods (`app.list`, `app.install`, `host.devices.list`, ...) and the host's notifications (`window.open`,
 * `app.installed`, ...). Rather than a second socket, this wraps the one transport:
 *
 *   - the shell's requests carry string ids (`s1`, `s2`, ...), the session's are numbers, so the two never
 *     meet: responses to a string id are the shell's and never reach the session;
 *   - every notification reaches the session (it ignores what it does not know) and the shell's listeners;
 */
import { createRpcPeer, RpcError } from './rpc/peer'
import type { Transport } from './rpc/transport'

type Params = Record<string, unknown>

/** How a call waits for its answer. */
export interface CallOptions {
  /** Give up after this long (default: the channel's); 0 waits for the host's answer or the end of the connection. */
  timeoutMs?: number
  /** Cancel the call: it ends here, and the host is told so it stops the handler and frees its place. */
  signal?: AbortSignal
}

export interface HostChannel {
  /** Hand this to the `Session`: it never sees the shell's responses. */
  readonly transport: Transport
  /** A host-management request (`app.list`, ...). Rejects with the host's message on a JSON-RPC error. */
  call(method: string, params?: Params, options?: CallOptions): Promise<unknown>
  /** Host notifications that no window owns. */
  onNotification(listener: (method: string, params: Params) => void): () => void
}

/** A host or connection error from a shell request. `code` is the JSON-RPC code. */
export class HostError extends Error {
  constructor(
    readonly code: number,
    message: string,
    readonly data?: unknown
  ) {
    super(message)
    this.name = 'HostError'
  }
}

const SHELL_ID = /^s\d+$/

/** Which side of the channel a raw frame belongs to. */
type Owner = 'shell-response' | 'notification' | 'session'

const ownerOf = (raw: string): Owner => {
  try {
    const frame = JSON.parse(raw) as { id?: unknown; method?: unknown }
    if (typeof frame !== 'object' || frame === null) return 'session'
    if (typeof frame.id === 'string' && SHELL_ID.test(frame.id) && frame.method === undefined)
      return 'shell-response'
    if (frame.id === undefined && typeof frame.method === 'string') return 'notification'
  } catch {
    // not JSON: the session reports it
  }
  return 'session'
}

/** A request that failed on the wire is a host error with the same code and message. */
const asHostError = (e: unknown): unknown =>
  e instanceof RpcError ? new HostError(e.code, e.message, e.data) : e

export function createHostChannel(base: Transport, timeoutMs = 30_000): HostChannel {
  const notifications = new Set<(method: string, params: Params) => void>()
  // The shell's requests are `s1`, `s2`, ... (the session's are numbers), so each peer only ever settles its own.
  const peer = createRpcPeer({
    send: (m) => {
      try {
        base.send(m)
      } catch {
        throw new HostError(-32000, 'Not connected to vos')
      }
    },
    onNotification: (method, params) => {
      for (const l of [...notifications]) l(method, params)
    },
    onError: (e) => console.warn('[vos] host channel:', e),
    timeoutMs,
    idPrefix: 's'
  })

  base.onMessage((raw) => {
    // the peer settles the shell's responses and fans out notifications; everything else is the session's
    if (ownerOf(raw) !== 'session') peer.receive(raw)
  })
  base.onStatus((status) => {
    if (status !== 'open') peer.failPending('The connection to vos closed')
  })

  const transport: Transport = {
    get status() {
      return base.status
    },
    send: (m) => base.send(m),
    connect: () => base.connect(),
    close: () => base.close(),
    retryNow: () => base.retryNow(),
    onStatus: (l) => base.onStatus(l),
    // the session sees every frame except the responses to the shell's requests
    onMessage: (l) =>
      base.onMessage((raw) => {
        if (ownerOf(raw) !== 'shell-response') l(raw)
      })
  }

  return {
    transport,
    call(method: string, params: Params = {}, options?: CallOptions): Promise<unknown> {
      return peer.request<unknown>(method, params, options).catch((e: unknown) => {
        throw asHostError(e)
      })
    },
    onNotification(listener) {
      notifications.add(listener)
      return () => void notifications.delete(listener)
    }
  }
}

/**
 * A host call that waits for the connection, for a little while: a page whose frame may ask before its socket is open (the
 * standalone page) calls through this. The options (`timeoutMs`, `signal`) are the caller's and reach `channel.call` whichever way the
 * call goes, so an `app.call` has the same lifetime on the standalone page as in a window.
 */
export function callWhenConnected(
  channel: Pick<HostChannel, 'transport' | 'call'>,
  method: string,
  params: Params,
  options?: CallOptions,
  waitMs = 10_000
): Promise<unknown> {
  if (channel.transport.status === 'open') return channel.call(method, params, options)
  return new Promise<unknown>((resolve, reject) => {
    const timer = setTimeout(() => {
      stop()
      reject(new HostError(-32000, 'Not connected to vos'))
    }, waitMs)
    const stop = channel.transport.onStatus((status) => {
      if (status !== 'open') return
      stop()
      clearTimeout(timer)
      channel.call(method, params, options).then(resolve, reject)
    })
  })
}
