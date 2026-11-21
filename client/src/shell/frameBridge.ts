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
import type { HostAppEvent } from '../view/protocol'

/** The longest method name a client may call (the backend's own limit, `MAX_HANDLER_NAME`). */
const MAX_METHOD = 128

const fromFrame = z.discriminatedUnion('t', [
  z.object({
    t: z.literal('rpc'),
    id: z.number().int().nonnegative(),
    method: z.string().min(1).max(MAX_METHOD),
    args: z.unknown().optional()
  }),
  z.object({ t: z.literal('ready') }),
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
  /** A host call (`/rpc`): the shell's own connection. */
  call(method: string, params: Record<string, unknown>): Promise<unknown>
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

export function createFrameBridge(options: FrameBridgeOptions): FrameBridge {
  const { appId } = options
  let disposed = false
  const post = (message: ToFrame): void => {
    if (!disposed) options.post(message)
  }

  return {
    receive({ source, data }) {
      const frame = options.frame()
      if (disposed || frame === null || source !== frame) return
      const parsed = fromFrame.safeParse(data)
      if (!parsed.success) return
      const message = parsed.data
      switch (message.t) {
        case 'rpc':
          options
            .call('app.call', { app: appId, method: message.method, args: message.args ?? null })
            .then(
              (result) => post({ t: 'res', id: message.id, result: result ?? null }),
              (error: unknown) => post({ t: 'res', id: message.id, error: messageOf(error) })
            )
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
