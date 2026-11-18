/**
 * The shell's side channel on the session's one connection.
 *
 * The renderer's `Session` speaks the view protocol (`view.*`, `data.*`, ...) and nothing else. The shell also
 * needs the host's own methods (`app.list`, `app.install`, `host.devices.list`, ...) and the notifications no
 * window owns (`window.open`, `app.installed`, ...). Rather than a second socket, this wraps the one transport:
 *
 *   - the shell's requests carry string ids (`s1`, `s2`, ...), the session's are numbers, so the two never
 *     meet: responses to a string id are the shell's and never reach the session;
 *   - every notification reaches the session (it ignores what it does not know) and the shell's listeners;
 *   - a toast with an action button is the shell's to show (`onToast`): the renderer's toasts are text only.
 *     Tapping the button is an ordinary `view.event` on `$toast/<id>` (see `Toast` in the protocol).
 */
import type { SessionOptions } from '../view'

type Transport = SessionOptions['transport']
type Params = Record<string, unknown>

export interface ToastWithAction {
  viewId: string
  text: string
  kind?: 'info' | 'success' | 'error'
  duration?: number
  action: { title: string; id: string }
}

export interface HostChannel {
  /** Hand this to the `Session`: it never sees the shell's responses or action toasts. */
  readonly transport: Transport
  /** A host-management request (`app.list`, ...). Rejects with the host's message on a JSON-RPC error. */
  call<T>(method: string, params?: Params): Promise<T>
  /** Host notifications that no window owns. */
  onNotification(listener: (method: string, params: Params) => void): () => void
  /** Toasts with an action button, for every window. */
  onToast(listener: (toast: ToastWithAction) => void): () => void
}

/** A host or connection error from a shell request. `code` is the JSON-RPC code. */
export class HostError extends Error {
  constructor(
    readonly code: number,
    message: string
  ) {
    super(message)
    this.name = 'HostError'
  }
}

const SHELL_ID = /^s\d+$/

interface Frame {
  id?: unknown
  method?: string
  params?: Params
  result?: unknown
  error?: { code: number; message: string }
}

const parse = (raw: string): Frame | null => {
  try {
    const frame = JSON.parse(raw) as unknown
    return typeof frame === 'object' && frame !== null ? (frame as Frame) : null
  } catch {
    return null
  }
}

export function createHostChannel(base: Transport, timeoutMs = 30_000): HostChannel {
  const pending = new Map<
    string,
    { resolve(v: unknown): void; reject(e: Error): void; timer: ReturnType<typeof setTimeout> }
  >()
  const notifications = new Set<(method: string, params: Params) => void>()
  const toasts = new Set<(toast: ToastWithAction) => void>()
  let next = 1

  const failAll = (message: string): void => {
    for (const [id, p] of pending) {
      clearTimeout(p.timer)
      p.reject(new HostError(-32000, message))
      pending.delete(id)
    }
  }

  const asToast = (f: Frame): ToastWithAction | null => {
    if (f.method !== 'view.command' || f.params?.cmd !== 'toast') return null
    const args = f.params.args as Partial<ToastWithAction> & { text?: string }
    if (!args?.action || typeof args.text !== 'string') return null
    return {
      viewId: String(f.params.viewId),
      text: args.text,
      kind: args.kind,
      duration: args.duration,
      action: args.action
    }
  }

  base.onMessage((raw) => {
    const frame = parse(raw)
    if (!frame) return
    if (typeof frame.id === 'string' && SHELL_ID.test(frame.id)) {
      const entry = pending.get(frame.id)
      if (!entry) return
      pending.delete(frame.id)
      clearTimeout(entry.timer)
      if (frame.error) entry.reject(new HostError(frame.error.code, frame.error.message))
      else entry.resolve(frame.result)
      return
    }
    if (frame.method && frame.id === undefined) {
      const toast = asToast(frame)
      if (toast) for (const l of [...toasts]) l(toast)
      else for (const l of [...notifications]) l(frame.method, frame.params ?? {})
    }
  })
  base.onStatus((status) => {
    if (status !== 'open') failAll('The connection to AgentOS closed')
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
    // the session sees every frame except the responses to the shell's requests and action toasts
    onMessage: (l) =>
      base.onMessage((raw) => {
        const frame = parse(raw)
        if (frame && typeof frame.id === 'string' && SHELL_ID.test(frame.id)) return
        if (frame && asToast(frame)) return
        l(raw)
      })
  }

  return {
    transport,
    call<T>(method: string, params: Params = {}): Promise<T> {
      const id = `s${next++}`
      return new Promise<T>((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(id)
          reject(new HostError(-32001, `"${method}" timed out`))
        }, timeoutMs)
        pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer })
        try {
          base.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }))
        } catch {
          pending.delete(id)
          clearTimeout(timer)
          reject(new HostError(-32000, 'Not connected to AgentOS'))
        }
      })
    },
    onNotification(listener) {
      notifications.add(listener)
      return () => void notifications.delete(listener)
    },
    onToast(listener) {
      toasts.add(listener)
      return () => void toasts.delete(listener)
    }
  }
}
