/**
 * The shell's side of an app's frame (the other side is `crates/vos-http/src/frame/bridge.js`, served into the frame).
 *
 * An app with a client runs in an `<iframe sandbox="allow-scripts">` of its own. The frame can only `postMessage`
 * to the shell, and this decides what to do with what arrives:
 *
 *   rpc    `{ t: 'rpc', id, method, args }`   → `app.call { app, method, args }` on `/rpc`, answered with
 *                                              `{ t: 'res', id, result | error }`
 *   ready  `{ t: 'ready' }`                   the client mounted
 *   failed `{ t: 'failed', message }`         the client could not start
 *
 * and to the frame: `{ t: 'event', name, data }` (an event the app's backend sent, or the host's own `host:*`).
 *
 * **The app is the frame's, never the message's.** Each frame has a bridge made for its own app id and its own
 * `contentWindow`: a message is only the frame's when `event.source` is that window, and `app.call` is always sent
 * with that bridge's app id. A message has no field that names an app, and one that carries an `app` is not read.
 * This is how apps do not trip over each other, not a defence against a hostile client.
 */
import { z } from 'zod'
import { messageOf } from '../view/errors'
import { callGate, MAX_WAITING_PER_FRAME, type CallGate } from './callGate'
import type { HostAppEvent } from '../view/protocol'

/** The longest method name a client may call (the backend's own limit, `MAX_HANDLER_NAME`). */
const MAX_METHOD = 128

/**
 * The most a call's arguments may weigh as JSON, in bytes. The host closes a connection that sends one WebSocket
 * message over 1 MiB, and the shell's connection is every window's, so one client's call must never get that far: the
 * bridge refuses it with an error first. (A result may be 4 MiB; it comes back as an error when it is more.)
 */
export const MAX_ARGS_BYTES = 1_000_000

/** The deepest nesting of arrays and objects in a call's arguments (the host's JSON parser stops at 128). */
export const MAX_ARGS_DEPTH = 100

const fromFrame = z.discriminatedUnion('t', [
  z.object({
    t: z.literal('rpc'),
    id: z.number().int().nonnegative(),
    method: z.string(),
    args: z.unknown().optional()
  }),
  z.object({ t: z.literal('ready') }),
  z.object({ t: z.literal('hello') }),
  z.object({ t: z.literal('failed'), message: z.string().max(1000) })
])

/** What the shell sends into a frame. */
export type ToFrame =
  | { t: 'res'; id: number; result: unknown }
  | { t: 'res'; id: number; error: string }
  | { t: 'event'; name: string; data: unknown }

export interface FrameBridgeOptions {
  /** The app this frame belongs to: fixed when the frame is made. */
  appId: string
  /** The frame's own window (`iframe.contentWindow`); `null` before the frame exists. */
  frame(): unknown
  /** Send a message into the frame. */
  post(message: ToFrame): void
  /**
   * A host call (`/rpc`): the shell's own connection. `options.timeoutMs` is 0 for `app.call`: the host keeps running a call
   * until its handler answers, and the gate counts it for as long, so the client must not give up before the host does.
   */
  call(
    method: string,
    params: Record<string, unknown>,
    options?: { timeoutMs?: number }
  ): Promise<unknown>
  /**
   * The frame's document is the host's own bridge script running: it says so (`hello`) the moment it starts, before the
   * client does anything, so a document that never does is a page the frame navigated itself to.
   */
  onHello?(): void
  /**
   * The window has given the frame up (the app left its page, or never started): nothing the frame posts is acted on while this is
   * true, so a page the app navigated to has no say over its window. Trying again makes a new frame and a new bridge.
   */
  muted?(): boolean
  /** Who may have a call in flight: the gate the frames of the shell share (default). */
  gate?: CallGate
  /** The client mounted. */
  onReady?(): void
  /** The client could not start. */
  onFailed?(message: string): void
}

export interface FrameBridge {
  /** A `message` event of the page: acts on it when it came from this frame, ignores it otherwise. */
  receive(event: { source: unknown; data: unknown }): void
  /** Send the frame an event (`host:theme`, or one the backend emitted). */
  event(name: string, data: unknown): void
  /** The frame is going away: answers still on their way are dropped. */
  dispose(): void
}

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

export function createFrameBridge(options: FrameBridgeOptions): FrameBridge {
  const { appId } = options
  let disposed = false
  const gate = options.gate ?? callGate
  /** What identifies this frame to the gate: its app, so a window closed and opened again meets the places still taken. */
  const frameKey = appId
  const post = (message: ToFrame): void => {
    if (!disposed) options.post(message)
  }

  return {
    receive({ source, data }) {
      const frame = options.frame()
      if (disposed || frame === null || source !== frame || options.muted?.()) return
      const parsed = fromFrame.safeParse(data)
      if (!parsed.success) {
        // a call that cannot be read still has an id to answer, so the client's promise settles
        const sent = data as { t?: unknown; id?: unknown } | null
        if (sent?.t === 'rpc' && Number.isInteger(sent.id) && (sent.id as number) >= 0) {
          post({
            t: 'res',
            id: sent.id as number,
            error: 'That is not a call: it needs a method name'
          })
        }
        return
      }
      const message = parsed.data
      switch (message.t) {
        case 'rpc': {
          if (message.method.length < 1 || message.method.length > MAX_METHOD) {
            post({
              t: 'res',
              id: message.id,
              error: `A method name is 1 to ${MAX_METHOD} characters`
            })
            return
          }
          const refusal = refuseArgs(message.args)
          if (refusal) {
            post({ t: 'res', id: message.id, error: refusal })
            return
          }
          void gate
            .enter(frameKey, () => disposed)
            .then((release) => {
              if (release === null) {
                post({
                  t: 'res',
                  id: message.id,
                  error: `Too many calls are waiting (at most ${MAX_WAITING_PER_FRAME}): let some finish first`
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
                  { app: appId, method: message.method, args: message.args ?? null },
                  { timeoutMs: 0 }
                )
                .then(
                  (result) => post({ t: 'res', id: message.id, result: result ?? null }),
                  (error: unknown) => post({ t: 'res', id: message.id, error: messageOf(error) })
                )
                .finally(release)
            })
          return
        }
        case 'hello':
          options.onHello?.()
          return
        case 'ready':
          options.onReady?.()
          return
        case 'failed':
          options.onFailed?.(message.message)
          return
      }
    },
    event(name, data) {
      post({ t: 'event', name, data })
    },
    dispose() {
      disposed = true
    }
  }
}

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
