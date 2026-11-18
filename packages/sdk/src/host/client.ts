/**
 * Client (PWA shell) side: the bridge to the server, and the runtime that
 * turns it into per-app `AppClientApi`s (scoped invoke, queued events, tools).
 */
import type { AppClientApi } from '../app/client'
import type { HostBridge } from './bridge'

export type { ConnectionStatus, HostBridge, Unsubscribe } from './bridge'
export { BRIDGE_GLOBAL, installHostBridge } from './bridge'
export { createWsHostBridge, type WsHostBridgeOptions } from './ws-bridge'
export type * from './protocol'

/** The installed bridge, or undefined before the shell installs one. */
export function hostBridge(): HostBridge | undefined {
  // globalThis (not window) so this also runs in Bun and workers.
  return (globalThis as { agentos?: HostBridge }).agentos
}

export interface AppRuntime {
  /** The SDK API handed to an app module's renderer. */
  apiFor(appId: string): AppClientApi
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
