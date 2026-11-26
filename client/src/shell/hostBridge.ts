/**
 * The host page's side of an app's frame (the other side is `crates/vos-http/src/apps/bridge.js`, which the app's page
 * loads). The shell and the standalone wrapper both make one of these per frame, over the same
 * {@link ServiceRegistry}.
 *
 * The frame can only `postMessage` to its host page, as JSON-RPC 2.0 in a dialect of MCP Apps (SEP-1865):
 *
 *   `ui/initialize`                 the handshake: answered with `hostCapabilities`, `hostInfo` and `hostContext`
 *   `ui/notifications/initialized`  the page is up (the host's cue to show it)
 *   `ui/...`, `vos/...`             a host service of the registry, if the host offers it
 *   any other method                "method not found": a page reaches its backend through the bus, `vos/request`
 *   `vos/request`                   a request to another app's backend (`{ app, method, params }`): forwarded as
 *                                   `app.call` for that app, held to the same limits as the frame's own calls
 *   `vos/failed`                    the page could not start
 *   `vos/shortcut`                  the shell's search chord was pressed in the frame
 *   `vos/telemetry`                 what the frame recorded (its calls' spans, its errors): passed on as `host.telemetry.ingest`
 *   `vos/telemetry/query`           the Telemetry app reads what was recorded (that app's frame only; `vos.tracing.query`)
 *   `vos/subscribe`                 the page listens to another app's public notifications (`{ app }`)
 *   `vos/handle`, `vos/unhandle`    the page serves (or stops serving) one of its app's methods on the RPC bus
 *   `vos/answer`                    the page's answer to a request of the bus (`{ call, result }` or `{ call, error }`):
 *                                   the host takes the first answer and refuses a late one as expired
 *   `vos/notify`                    the page sends one of its app's notifications on the bus (`{ name, params }`)
 *
 * and to the frame: `ui/notifications/host-context-changed` (a change of the host context), `vos/event` (what the
 * app's backend sent with `RPC.send_notification`, and the public notifications of the apps it subscribed to, with their `app`),
 * `vos/serve` (a request of the bus for a method the page serves) and `vos/settled` (another handler answered it) and the request `ui/resource-teardown` (the frame is about to go).
 *
 * **The app is the frame's, never the message's.** Each frame has a bridge made for its own app id and its own
 * `contentWindow`: a message is only the frame's when `event.source` is that window, and `app.call` is always sent
 * with that bridge's app id. A message has no field that names an app, and one that carries an `app` is not read.
 * This is how apps do not trip over each other, not a defence against a hostile client.
 */
import { z } from 'zod'
import { messageOf } from './errors'
import { callGate, MAX_WAITING_PER_FRAME, type CallGate } from './callGate'
import { HostError } from './channel'
import type { HostContext } from './hostContext'
import type { ServiceRegistry } from './hostServices'
import type { HostAppEvent } from './protocol'

/** The version of MCP Apps' dialect this host speaks. */
export const PROTOCOL_VERSION = '2026-01-26'

/** The longest method name a client may call (the backend's own limit, `MAX_HANDLER_NAME`). */
const MAX_METHOD = 128

/**
 * The most a call's arguments may weigh as JSON, in bytes. The host closes a connection that sends one WebSocket message
 * over 1 MiB, and the host page's connection is every window's, so one app's call must never get that far: the bridge
 * refuses it with an error first. (A result may be 4 MiB; it comes back as an error when it is more.)
 */
export const MAX_ARGS_BYTES = 1_000_000

/** The deepest nesting of arrays and objects in a call's arguments (the host's JSON parser stops at 128). */
const MAX_ARGS_DEPTH = 100

const id = z.union([z.number().int().nonnegative(), z.string().max(64)])
const method = z.string()

/** A call's trace context (NEOC-521 POC): `{ traceparent }`, next to `params` so the app's arguments stay its own. */
const meta = z.object({ traceparent: z.string().max(64).optional() }).optional()

const message = z.union([
  z.object({ jsonrpc: z.literal('2.0'), id, method, params: z.unknown().optional(), _meta: meta }),
  z.object({ jsonrpc: z.literal('2.0'), method, params: z.unknown().optional() }),
  z
    .object({
      jsonrpc: z.literal('2.0'),
      id,
      result: z.unknown().optional(),
      error: z.object({ code: z.number(), message: z.string() }).passthrough().optional()
    })
    .refine((m) => 'result' in m || 'error' in m) // an answer says which
])

export interface RpcError {
  code: number
  message: string
  data?: unknown
}

/** What a host page sends into a frame. */
export type ToFrame =
  | { jsonrpc: '2.0'; id: number | string; result: unknown }
  | { jsonrpc: '2.0'; id: number | string; error: RpcError }
  | { jsonrpc: '2.0'; id: number; method: string; params: unknown }
  | { jsonrpc: '2.0'; method: string; params: unknown }

export interface HostBridgeOptions {
  /** The app this frame belongs to: fixed when the frame is made. */
  appId: string
  /** The frame's own window (`iframe.contentWindow`); `null` before the frame exists. */
  frame(): unknown
  /** Send a message into the frame. */
  post(message: ToFrame): void
  /**
   * A host call (`/rpc`): the page's own connection. `options.timeoutMs` is 0 for `app.call`: the host keeps running a call until
   * its handler answers, and the gate counts it for as long, so the client must not give up before the host does.
   */
  call(
    method: string,
    params: Record<string, unknown>,
    options?: { timeoutMs?: number; signal?: AbortSignal }
  ): Promise<unknown>
  /**
   * The window has given the frame up (the app left its page, or never started): nothing the frame posts is acted on while this is
   * true, so a page the app navigated to has no say over its window. Trying again makes a new frame and a new bridge.
   */
  muted?(): boolean
  /** What the page offers. */
  services: ServiceRegistry
  /** The host context now: the handshake's answer. */
  context(): HostContext
  /**
   * The page's bridge script started: its `ui/initialize` is the first thing it says, synchronously, before any app code runs, so
   * a document that never says it is a page the frame navigated itself to.
   */
  onHello?(): void
  /** Who may have a call in flight: the gate the frames of the shell share (default). */
  gate?: CallGate
  /** The page told the host it is up. */
  onReady?(): void
  /** The page could not start. */
  onFailed?(message: string): void
  /** A chord of the shell's own was pressed while the keys were in the frame (`vos/shortcut`). */
  onShortcut?(chord: { key: string; metaKey: boolean; ctrlKey: boolean }): void
}

export interface HostBridge {
  /** A `message` event of the page: acts on it when it came from this frame, ignores it otherwise. */
  receive(event: { source: unknown; data: unknown }): void
  /** The host context changed: tell the frame what is different (`ui/notifications/host-context-changed`). */
  contextChanged(change: Partial<HostContext>): void
  /** The app's backend sent an event: tell the frame (`vos/event`). */
  event(name: string, data: unknown): void
  /**
   * Another app's backend sent an event: tell the frame (`vos/event` with its `app`) when the frame subscribed to that app
   * and the event is public. A `_name` is that app's own page's alone, and a `host:` name is the host's: neither leaves.
   */
  appEvent(app: string, name: string, data: unknown): void
  /** A request of the bus for one of the app's methods (`app.request`): to the frame if it serves the method. */
  serve(call: number, method: string, params: unknown): void
  /** Another handler answered the request (`app.requestSettled`): the frame drops what it shows for it. */
  settled(call: number): void
  /** The connection came back: the host forgot the frame's handlers, so they are registered again. */
  rehandle(): void
  /** Ask the frame to wrap up before it is removed (`ui/resource-teardown`); settles when it answers or `ms` pass. */
  teardown(reason: string, ms?: number): Promise<void>
  /** The frame is going away: answers still on their way are dropped. */
  dispose(): void
}

/** JSON-RPC's codes. */
const INVALID_REQUEST = -32600
/** A call the bridge refuses because the frame already has too many waiting: the host's own busy code (`codes::BUSY`, -32000). */
const BUSY = -32000
const METHOD_NOT_FOUND = -32601
const INVALID_PARAMS = -32602
const INTERNAL_ERROR = -32603

/** The app whose frame may read what was recorded (`vos/telemetry/query`): what covers every app is not every app's to read. */
export const TELEMETRY_APP = 'telemetry'
/** The most a frame's `vos/telemetry` batch may weigh as JSON, in bytes; a heavier one is dropped. */
const MAX_TELEMETRY_BYTES = 256_000
const TRACEPARENT = /^00-[0-9a-f]{32}-[0-9a-f]{16}-[0-9a-f]{2}$/

/** The nesting depth of a JSON text: arrays and objects opened and not yet closed, outside strings. */
export function jsonDepth(text: string): number {
  let depth = 0
  let deepest = 0
  let inString = false
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (inString) {
      if (c === '\\') i++
      else if (c === '"') inString = false
    } else if (c === '"') inString = true
    else if (c === '[' || c === '{') deepest = Math.max(deepest, ++depth)
    else if (c === ']' || c === '}') depth--
  }
  return deepest
}

/** Why a call's arguments cannot be sent, or `null` when they can. */
export function refuseArgs(args: unknown): string | null {
  if (args === undefined || args === null) return null
  let json: string | undefined
  try {
    json = JSON.stringify(args)
  } catch (error) {
    return `The arguments cannot be sent: ${messageOf(error)}`
  }
  if (json === undefined) return 'The arguments cannot be sent'
  // up to three bytes a character: measure exactly only when it could matter
  const bytes =
    json.length * 3 > MAX_ARGS_BYTES ? new TextEncoder().encode(json).length : json.length
  if (bytes > MAX_ARGS_BYTES) {
    return `The arguments are ${bytes.toLocaleString('en')} bytes; a call may carry at most ${MAX_ARGS_BYTES.toLocaleString('en')}`
  }
  if (jsonDepth(json) > MAX_ARGS_DEPTH) {
    return `The arguments are nested more than ${MAX_ARGS_DEPTH} levels deep`
  }
  return null
}

/** The JSON-RPC error a failed call answers with: the host's code, message and data, else an internal error. */
function rpcErrorOf(error: unknown): RpcError {
  if (error instanceof HostError) {
    return {
      code: error.code,
      message: error.message,
      ...(error.data === undefined ? {} : { data: error.data })
    }
  }
  return { code: INTERNAL_ERROR, message: messageOf(error) }
}

const initializeParams = z.object({
  protocolVersion: z.string(),
  appCapabilities: z.record(z.string(), z.unknown())
})

export function createHostBridge(options: HostBridgeOptions): HostBridge {
  const { appId, services } = options
  let disposed = false
  /** What the calls this frame has running on the host share: aborted when the frame goes, so the host stops them. */
  const running = new AbortController()
  const gate = options.gate ?? callGate
  /** What identifies this frame to the gate: its app, so a window closed and opened again meets the places still taken. */
  const frameKey = appId
  let nextId = 1
  const waiting = new Map<number | string, () => void>()
  /** The other apps whose public notifications the frame asked for (`vos/subscribe`). */
  const subscriptions = new Set<string>()
  /** The app's methods the frame serves on the bus (`vos/handle`), and the requests it was handed, by call id. */
  const handled = new Set<string>()
  const served = new Set<number>()
  /** Tell the host, without waiting: a lost registration is made again when the connection comes back. */
  const tellHost = (method: string, params: Record<string, unknown>): void => {
    options.call(method, params, { timeoutMs: 10_000 }).catch(() => {})
  }
  const post = (m: ToFrame): void => {
    if (!disposed) options.post(m)
  }
  const answer = (callId: number | string, run: () => unknown): void => {
    Promise.resolve()
      .then(run)
      .then(
        (result) => post({ jsonrpc: '2.0', id: callId, result: result ?? null }),
        (error: unknown) => post({ jsonrpc: '2.0', id: callId, error: rpcErrorOf(error) })
      )
  }

  const request = (
    callId: number | string,
    name: string,
    params: unknown,
    traceparent?: string
  ): void => {
    if (name.length < 1 || name.length > MAX_METHOD) {
      post({
        jsonrpc: '2.0',
        id: callId,
        error: { code: INVALID_REQUEST, message: `A method name is 1 to ${MAX_METHOD} characters` }
      })
      return
    }
    if (name === 'ui/initialize') {
      options.onHello?.()
      const parsed = initializeParams.safeParse(params)
      if (!parsed.success) {
        post({
          jsonrpc: '2.0',
          id: callId,
          error: {
            code: INVALID_PARAMS,
            message: 'ui/initialize needs protocolVersion and appCapabilities'
          }
        })
        return
      }
      post({
        jsonrpc: '2.0',
        id: callId,
        result: {
          protocolVersion: PROTOCOL_VERSION,
          hostInfo: { name: 'vos', version: '1' },
          hostCapabilities: services.capabilities(),
          hostContext: options.context()
        }
      })
      return
    }
    if (name === 'ping') {
      post({ jsonrpc: '2.0', id: callId, result: {} })
      return
    }
    if (name === 'vos/request') {
      otherAppRequest(callId, params, traceparent)
      return
    }
    if (name === 'vos/handle' || name === 'vos/unhandle' || name === 'vos/answer' || name === 'vos/notify') {
      busMessage(callId, name, params)
      return
    }
    if (name === 'vos/telemetry/query' && appId === TELEMETRY_APP) {
      const p = (typeof params === 'object' && params !== null ? params : {}) as Record<string, unknown>
      const { kind, ...rest } = p
      answer(callId, () =>
        options.call(kind === 'traces' ? 'host.telemetry.traces' : 'host.telemetry.query', rest)
      )
      return
    }
    const service = services.get(name)
    if (service) {
      answer(callId, () => service(params, { appId }))
      return
    }
    // anything else is not this host's: MCP's and the dialect's own names are never an app's, and a page reaches its
    // backend (and another app's) through the bus, `vos/request`
    post({
      jsonrpc: '2.0',
      id: callId,
      error: { code: METHOD_NOT_FOUND, message: `This host does not offer ${name}` }
    })
  }

  /** The page's side of the RPC bus: its handlers, its answers and its notifications, always for its own app. */
  const busMessage = (callId: number | string, name: string, params: unknown): void => {
    const p = (typeof params === 'object' && params !== null ? params : {}) as Record<string, unknown>
    const fail = (message: string): void => post({ jsonrpc: '2.0', id: callId, error: { code: INVALID_PARAMS, message } })
    if (name === 'vos/handle' || name === 'vos/unhandle') {
      const method = p.method
      if (typeof method !== 'string' || method.length < 1 || method.length > MAX_METHOD) return fail(`${name} needs a method name`)
      if (name === 'vos/handle') handled.add(method)
      else handled.delete(method)
      answer(callId, () => options.call(name === 'vos/handle' ? 'app.handle' : 'app.unhandle', { app: appId, method }, { timeoutMs: 10_000 }))
      return
    }
    if (name === 'vos/answer') {
      if (typeof p.call !== 'number') return fail('vos/answer needs the call it answers')
      served.delete(p.call)
      const body: Record<string, unknown> = { call: p.call }
      if (p.error !== undefined) body.error = p.error
      else body.result = p.result ?? null
      answer(callId, () => options.call('app.answer', body, { timeoutMs: 10_000 }))
      return
    }
    // vos/notify
    if (typeof p.name !== 'string' || p.name.length < 1) return fail('vos/notify needs the notification name')
    const refusal = refuseArgs(p.params)
    if (refusal) return fail(refusal)
    answer(callId, () => options.call('app.notify', { app: appId, name: p.name, params: p.params ?? {} }, { timeoutMs: 10_000 }))
  }

  /** Another app's request (`vos/request`): checked, then forwarded as that app's `app.call`. */
  const otherAppRequest = (callId: number | string, params: unknown, traceparent?: string): void => {
    const p = (typeof params === 'object' && params !== null ? params : {}) as { app?: unknown; method?: unknown; params?: unknown }
    const fail = (message: string): void => post({ jsonrpc: '2.0', id: callId, error: { code: INVALID_PARAMS, message } })
    if (typeof p.app !== 'string' || !APP_ID.test(p.app)) return fail('vos/request needs the id of the app to call (`app`)')
    if (typeof p.method !== 'string' || p.method.length < 1 || p.method.length > MAX_METHOD || p.method.includes('/'))
      return fail(`vos/request needs the name of a method of ${p.app} (\`method\`)`)
    forward(callId, p.app, p.method, p.params, traceparent)
  }

  /** A call of an app's backend, through the gate: the frame's own app's, or (`vos/request`) another app's. */
  const forward = (callId: number | string, app: string, name: string, params: unknown, traceparent?: string): void => {
    const refusal = refuseArgs(params)
    if (refusal) {
      post({ jsonrpc: '2.0', id: callId, error: { code: INVALID_PARAMS, message: refusal } })
      return
    }
    void gate
      .enter(frameKey, () => disposed)
      .then((release) => {
        if (release === null) {
          post({
            jsonrpc: '2.0',
            id: callId,
            error: {
              code: BUSY,
              message: `Too many calls are waiting (at most ${MAX_WAITING_PER_FRAME}): let some finish first`
            }
          })
          return
        }
        if (disposed) {
          release()
          return
        }
        options
          .call(
            'app.call',
            {
              app,
              method: name,
              args: params ?? null,
              ...(traceparent && TRACEPARENT.test(traceparent) ? { traceparent } : {})
            },
            { timeoutMs: 0, signal: running.signal }
          )
          .then(
            (result) => post({ jsonrpc: '2.0', id: callId, result: result ?? null }),
            (error: unknown) => post({ jsonrpc: '2.0', id: callId, error: rpcErrorOf(error) })
          )
          .finally(release)
      })
  }

  const notification = (name: string, params: unknown): void => {
    if (name === 'ui/notifications/initialized') options.onReady?.()
    else if (name === 'vos/failed') {
      const text =
        typeof params === 'object' && params !== null
          ? (params as { message?: unknown }).message
          : null
      options.onFailed?.(
        typeof text === 'string' ? text.slice(0, 1000) : 'The app could not start.'
      )
    } else if (name === 'vos/telemetry') {
      // what the frame recorded: passed on for this frame's app; a batch that is not one, or too heavy, is dropped
      const batch = typeof params === 'object' && params !== null ? (params as Record<string, unknown>) : null
      if (!batch || JSON.stringify(batch).length > MAX_TELEMETRY_BYTES) return
      const spans = Array.isArray(batch.spans) ? batch.spans : []
      const logs = Array.isArray(batch.logs) ? batch.logs : []
      if (spans.length + logs.length === 0) return
      // a failure to record is never the app's problem
      options.call('host.telemetry.ingest', { app: appId, spans, logs }).catch(() => {})
    } else if (name === 'vos/subscribe') {
      const app = typeof params === 'object' && params !== null ? (params as { app?: unknown }).app : null
      if (typeof app === 'string' && APP_ID.test(app) && app !== appId) subscriptions.add(app)
    } else if (name === 'vos/shortcut') {
      const chord =
        typeof params === 'object' && params !== null ? (params as Record<string, unknown>) : {}
      // only the one chord, whatever the page says: a page cannot type into the shell through this
      if (chord.key === 'k')
        options.onShortcut?.({
          key: 'k',
          metaKey: chord.metaKey === true,
          ctrlKey: chord.ctrlKey === true
        })
    }
    // `ui/notifications/size-changed`, `notifications/message`, and anything else: nothing to do for a window the person sizes
  }

  return {
    receive({ source, data }) {
      const frame = options.frame()
      if (disposed || frame === null || source !== frame || options.muted?.()) return
      const parsed = message.safeParse(data)
      if (!parsed.success) {
        // a request that cannot be read still has an id to answer, so the page's promise settles
        const sent = data as { jsonrpc?: unknown; id?: unknown; method?: unknown } | null
        if (
          sent?.jsonrpc === '2.0' &&
          sent.method !== undefined &&
          (typeof sent.id === 'string' || Number.isInteger(sent.id)) &&
          (typeof sent.id !== 'number' || sent.id >= 0)
        ) {
          post({
            jsonrpc: '2.0',
            id: sent.id as number | string,
            error: { code: INVALID_REQUEST, message: 'That is not a call: it needs a method name' }
          })
        }
        return
      }
      const m = parsed.data
      if ('method' in m) {
        if ('id' in m) request(m.id, m.method, m.params, '_meta' in m ? m._meta?.traceparent : undefined)
        else notification(m.method, m.params)
      } else {
        // an answer to a request the host made (`ui/resource-teardown`)
        waiting.get(m.id)?.()
      }
    },
    contextChanged(change) {
      if (Object.keys(change).length === 0) return
      post({ jsonrpc: '2.0', method: 'ui/notifications/host-context-changed', params: change })
    },
    event(name, data) {
      // the app's own event: with its app (as another app's carries), and marked as the frame's own; a host event
      // (`host:...`) is nobody's app's
      post({
        jsonrpc: '2.0',
        method: 'vos/event',
        params: name.startsWith('host:') ? { name, data } : { app: appId, own: true, name, data }
      })
    },
    serve(call, method, params) {
      if (!handled.has(method)) {
        // the frame stopped serving it before the request reached it: that is no answer
        tellHost('app.answer', { call, error: { code: METHOD_NOT_FOUND, message: `${appId}'s page no longer serves ${method}` } })
        return
      }
      served.add(call)
      post({ jsonrpc: '2.0', method: 'vos/serve', params: { call, method, params } })
    },
    settled(call) {
      if (!served.delete(call)) return
      post({ jsonrpc: '2.0', method: 'vos/settled', params: { call } })
    },
    rehandle() {
      for (const method of handled) tellHost('app.handle', { app: appId, method })
    },
    appEvent(app, name, data) {
      if (!subscriptions.has(app) || name.startsWith('_') || name.startsWith('host:')) return
      post({ jsonrpc: '2.0', method: 'vos/event', params: { app, name, data } })
    },
    teardown(reason, ms = 1000) {
      if (disposed) return Promise.resolve()
      const callId = nextId++
      return new Promise<void>((resolve) => {
        const timer = setTimeout(done, ms)
        function done(): void {
          clearTimeout(timer)
          waiting.delete(callId)
          resolve()
        }
        waiting.set(callId, done)
        post({ jsonrpc: '2.0', id: callId, method: 'ui/resource-teardown', params: { reason } })
      })
    },
    dispose() {
      // the frame no longer serves anything on the bus, and answers nothing it was handed
      for (const method of handled) tellHost('app.unhandle', { app: appId, method })
      for (const call of served) tellHost('app.answer', { call, error: { code: METHOD_NOT_FOUND, message: 'The window closed' } })
      handled.clear()
      served.clear()
      disposed = true
      // closing the window ends the calls the host is still running for it, and frees their places
      running.abort()
      for (const done of [...waiting.values()]) done()
    }
  }
}

/** An app id, as the host makes them. */
const APP_ID = /^[a-z0-9][a-z0-9-]{2,39}$/

/** The `app.event` notification of the host (`AppEvent` in the generated types): which app, and what its backend sent. */
const appEvent = z.object({
  app: z.string(),
  name: z.string(),
  data: z.json()
}) satisfies z.ZodType<HostAppEvent>

/** The event of an `app.event` notification when it is for `appId`, else `null`. */
export function eventFor(
  appId: string,
  method: string,
  params: Record<string, unknown>
): { name: string; data: HostAppEvent['data'] } | null {
  if (method !== 'app.event') return null
  const parsed = appEvent.safeParse(params)
  return parsed.success && parsed.data.app === appId
    ? { name: parsed.data.name, data: parsed.data.data }
    : null
}

/** The event of an `app.event` notification when it is another app's than `appId` (for {@link HostBridge.appEvent}), else `null`. */
export function otherAppEventFor(
  appId: string,
  method: string,
  params: Record<string, unknown>
): { app: string; name: string; data: HostAppEvent['data'] } | null {
  if (method !== 'app.event') return null
  const parsed = appEvent.safeParse(params)
  return parsed.success && parsed.data.app !== appId ? parsed.data : null
}

/**
 * Hand a host notification to the frame's bridge when it is the frame's: its app's events and the public events of the
 * apps it subscribed to, the requests of the bus for its app (`app.request`) and their settling (`app.requestSettled`).
 */
export function routeToFrame(bridge: HostBridge, appId: string, method: string, params: Record<string, unknown>): void {
  const event = eventFor(appId, method, params)
  if (event) return bridge.event(event.name, event.data)
  const other = otherAppEventFor(appId, method, params)
  if (other) return bridge.appEvent(other.app, other.name, other.data)
  if (method === 'app.request' && params.app === appId && typeof params.call === 'number' && typeof params.method === 'string')
    return bridge.serve(params.call, params.method, params.params)
  if (method === 'app.requestSettled' && typeof params.call === 'number') bridge.settled(params.call)
}
