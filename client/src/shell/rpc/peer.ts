/**
 * A transport-agnostic JSON-RPC 2.0 peer with **by-name params objects**. Both ends of
 * the socket run one: each side sends requests (awaited) and notifications (fire-and-forget) and handles the
 * other side's. `params` is always an object.
 */
import { messageOf } from '../errors'

export type JsonRpcId = number | string
export type Params = Record<string, unknown>

export interface JsonRpcErrorObject {
  code: number
  message: string
  data?: unknown
}

export const RpcErrorCode = {
  ParseError: -32700,
  InvalidRequest: -32600,
  MethodNotFound: -32601,
  InvalidParams: -32602,
  InternalError: -32603,
  /** Implementation-defined: the request was cancelled because the connection closed (or was never open). */
  ConnectionClosed: -32000,
  /** Implementation-defined: no response within the timeout. */
  Timeout: -32001,
  /** Implementation-defined: the caller cancelled the request (its signal was aborted). */
  Cancelled: -32800
} as const

/** An error that crosses the wire with a JSON-RPC code. */
export class RpcError extends Error {
  constructor(
    readonly code: number,
    message: string,
    readonly data?: unknown
  ) {
    super(message)
    this.name = 'RpcError'
  }
}

export interface RpcPeerOptions {
  /** Deliver one serialized message to the other side. */
  send(message: string): void
  /** Handle an incoming request; the return value (or thrown error) becomes the response. */
  onRequest?(method: string, params: Params): unknown | Promise<unknown>
  /** Handle an incoming notification. Errors are reported via `onError`. */
  onNotification?(method: string, params: Params): void
  /** Map a thrown handler error to a JSON-RPC error (default: RpcError as-is, else InternalError). */
  toRpcError?(error: unknown): JsonRpcErrorObject
  /** Problems that have no response to carry them (bad frames, notification handler errors). */
  onError?(error: unknown): void
  /** Reject an outgoing request after this many ms (default 60s; 0 = never). */
  timeoutMs?: number
  /**
   * Prefix of the ids this peer gives its requests (`s` makes `s1`, `s2`, ...; default none, so plain numbers). Two
   * peers sharing one connection use different prefixes and never mistake each other's responses.
   */
  idPrefix?: string
}

export interface RpcPeer {
  /**
   * `timeoutMs` overrides the peer's for this request (0 = wait for the answer or the end of the connection): a caller that
   * counts the request as running on the other side for as long as it is must not give up before the other side does.
   * `signal` cancels it: the request ends here with `Cancelled`, and the other side is told (`notifications/cancelled`, MCP's
   * cancellation) so it stops the work and frees what it holds. A request that times out is cancelled the same way.
   */
  request<T = unknown>(
    method: string,
    params?: Params,
    options?: { timeoutMs?: number; signal?: AbortSignal }
  ): Promise<T>
  notify(method: string, params?: Params): void
  /** Feed one raw message received from the other side. */
  receive(raw: string): void
  /** Reject every in-flight request (the socket dropped) but keep the peer: the connection may come back. */
  failPending(reason?: string): void
  /** Reject every in-flight request (the socket closed) and refuse new ones. */
  close(reason?: string): void
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

const defaultToRpcError = (error: unknown): JsonRpcErrorObject =>
  error instanceof RpcError
    ? { code: error.code, message: error.message, data: error.data }
    : {
        code: RpcErrorCode.InternalError,
        message: messageOf(error)
      }

interface Pending {
  resolve(v: unknown): void
  reject(e: RpcError): void
  timer?: ReturnType<typeof setTimeout>
}

export function createRpcPeer(options: RpcPeerOptions): RpcPeer {
  const pending = new Map<JsonRpcId, Pending>()
  const timeoutMs = options.timeoutMs ?? 60_000
  const idPrefix = options.idPrefix ?? ''
  const toRpcError = options.toRpcError ?? defaultToRpcError
  const report = (error: unknown): void => options.onError?.(error)
  let nextId = 1
  let closed = false

  const write = (message: object): void => {
    if (!closed) options.send(JSON.stringify(message))
  }
  const fail = (id: JsonRpcId | null, code: number, message: string): void =>
    write({ jsonrpc: '2.0', id, error: { code, message } })

  const respond = async (id: JsonRpcId, method: string, params: Params): Promise<void> => {
    if (!options.onRequest)
      return fail(id, RpcErrorCode.MethodNotFound, `Unknown method "${method}"`)
    try {
      const result = await options.onRequest(method, params)
      write({ jsonrpc: '2.0', id, result: result === undefined ? null : result })
    } catch (error) {
      write({ jsonrpc: '2.0', id, error: toRpcError(error) })
    }
  }

  const settle = (message: Record<string, unknown>): void => {
    const id = message.id as JsonRpcId
    const entry = pending.get(id)
    if (!entry) return report(new Error(`Response for unknown request id ${String(id)}`))
    pending.delete(id)
    if (entry.timer) clearTimeout(entry.timer)
    if ('error' in message && isObject(message.error)) {
      const e = message.error
      entry.reject(new RpcError(Number(e.code), String(e.message), e.data))
    } else entry.resolve(message.result)
  }

  /** Tell the other side a request is no longer wanted (best effort: a closed connection has nothing left to stop). */
  function cancelRemote(id: JsonRpcId, reason: string): void {
    try {
      write({
        jsonrpc: '2.0',
        method: 'notifications/cancelled',
        params: { requestId: id, reason }
      })
    } catch {
      // the connection is gone: whatever it ran is stopped with it
    }
  }

  function failPending(reason = 'Connection closed'): void {
    for (const [id, entry] of pending) {
      if (entry.timer) clearTimeout(entry.timer)
      entry.reject(new RpcError(RpcErrorCode.ConnectionClosed, reason))
      pending.delete(id)
    }
  }

  return {
    request<T>(
      method: string,
      params: Params = {},
      requestOptions?: { timeoutMs?: number; signal?: AbortSignal }
    ): Promise<T> {
      if (closed)
        return Promise.reject(new RpcError(RpcErrorCode.ConnectionClosed, 'Connection closed'))
      const id: JsonRpcId = idPrefix ? `${idPrefix}${nextId++}` : nextId++
      return new Promise<T>((resolve, reject) => {
        const entry: Pending = { resolve: resolve as (v: unknown) => void, reject }
        const limit = requestOptions?.timeoutMs ?? timeoutMs
        if (limit > 0) {
          entry.timer = setTimeout(() => {
            pending.delete(id)
            cancelRemote(id, 'timed out')
            reject(new RpcError(RpcErrorCode.Timeout, `"${method}" timed out after ${limit}ms`))
          }, limit)
        }
        const signal = requestOptions?.signal
        if (signal?.aborted) {
          if (entry.timer) clearTimeout(entry.timer)
          reject(new RpcError(RpcErrorCode.Cancelled, `"${method}" was cancelled`))
          return
        }
        signal?.addEventListener(
          'abort',
          () => {
            if (!pending.delete(id)) return // answered already
            if (entry.timer) clearTimeout(entry.timer)
            cancelRemote(id, 'cancelled')
            reject(new RpcError(RpcErrorCode.Cancelled, `"${method}" was cancelled`))
          },
          { once: true }
        )
        pending.set(id, entry)
        try {
          write({ jsonrpc: '2.0', id, method, params })
        } catch (error) {
          // the transport refused the frame (not open): no response will come, so forget the request
          pending.delete(id)
          if (entry.timer) clearTimeout(entry.timer)
          reject(
            new RpcError(
              RpcErrorCode.ConnectionClosed,
              error instanceof Error ? error.message : 'Not connected'
            )
          )
        }
      })
    },

    notify(method, params = {}) {
      write({ jsonrpc: '2.0', method, params })
    },

    receive(raw) {
      let message: unknown
      try {
        message = JSON.parse(raw)
      } catch {
        return fail(null, RpcErrorCode.ParseError, 'Parse error')
      }
      if (!isObject(message) || message.jsonrpc !== '2.0')
        return fail(null, RpcErrorCode.InvalidRequest, 'Invalid request')
      const hasId = typeof message.id === 'number' || typeof message.id === 'string'
      if (typeof message.method === 'string') {
        const params = message.params === undefined ? {} : message.params
        if (!isObject(params)) {
          if (hasId)
            fail(message.id as JsonRpcId, RpcErrorCode.InvalidParams, 'params must be an object')
          return
        }
        if (hasId) void respond(message.id as JsonRpcId, message.method, params)
        else {
          try {
            options.onNotification?.(message.method, params)
          } catch (error) {
            report(error)
          }
        }
        return
      }
      if (hasId && ('result' in message || 'error' in message)) return settle(message)
      report(new Error('Unrecognised JSON-RPC message'))
    },

    failPending,

    close(reason = 'Connection closed') {
      if (closed) return
      closed = true
      failPending(reason)
    }
  }
}
