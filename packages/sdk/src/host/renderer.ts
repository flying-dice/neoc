/**
 * Host-renderer side: access to the bridge, and the runtime that turns it
 * into per-app `AppRendererApi`s (scoped invoke, queued events, tools).
 */
import type { AppRendererApi } from '../app/renderer'
import type { HostBridge } from './bridge'

export type { HostBridge, Unsubscribe } from './bridge'
export type * from './protocol'

/** The preload bridge, or undefined outside the desktop host (e.g. a plain browser). */
export function hostBridge(): HostBridge | undefined {
  return typeof window === 'undefined' ? undefined : window.agentos
}

export interface AppRuntime {
  /** The SDK API handed to an app module's renderer. */
  apiFor(appId: string): AppRendererApi
}

const MAX_QUEUED = 20
const NO_HOST = 'This app needs the AgentOS desktop host'

/**
 * Creates the app runtime. Main → app events are routed per app; events that
 * arrive before an app window has subscribed (e.g. a tool opens the window
 * and immediately pushes data) are queued and replayed to the first listener.
 */
export function createAppRuntime(
  bridge: HostBridge | undefined,
  host: { openApp(appId: string): void }
): AppRuntime {
  type Listener = (payload: unknown) => void
  const listeners = new Map<string, Set<Listener>>()
  const queued = new Map<string, unknown[]>()
  const key = (appId: string, event: string): string => `${appId}\u0000${event}`

  bridge?.apps.onEvent(({ appId, event, payload }) => {
    const k = key(appId, event)
    const set = listeners.get(k)
    if (set?.size) for (const listener of set) listener(payload)
    else queued.set(k, [...(queued.get(k) ?? []), payload].slice(-MAX_QUEUED))
  })

  const subscribe = (appId: string, event: string, listener: Listener): (() => void) => {
    const k = key(appId, event)
    const set = listeners.get(k) ?? new Set<Listener>()
    listeners.set(k, set)
    set.add(listener)
    const pending = queued.get(k)
    if (pending) {
      queued.delete(k)
      queueMicrotask(() => pending.forEach((p) => listener(p)))
    }
    return () => set.delete(listener)
  }

  const unavailable = (): Promise<never> => Promise.reject(new Error(NO_HOST))

  return {
    apiFor: (appId) =>
      Object.freeze({
        appId,
        invoke: <T>(channel: string, payload?: unknown) =>
          bridge ? (bridge.apps.invoke(appId, channel, payload) as Promise<T>) : unavailable(),
        on: (event: string, listener: Listener) => subscribe(appId, event, listener),
        tools: Object.freeze({
          list: () => (bridge ? bridge.tools.list() : Promise.resolve([])),
          call: <T>(id: string, input?: Record<string, unknown>) =>
            bridge ? (bridge.tools.call(id, input) as Promise<T>) : unavailable()
        }),
        host: Object.freeze({ openApp: (id: string) => host.openApp(id) })
      })
  }
}
