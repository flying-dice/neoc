/**
 * Client side of an app module: built as a browser ESM bundle (`client.js`)
 * that the AgentOS PWA imports when the app's window first opens.
 *
 *   export default defineClientApp({ component: CalculatorPage })
 *
 * The bundle doesn't ship its own Svelte, kit or SDK: the build maps those to
 * the host's shared instances (see `@agentos/sdk/build` → SHARED_MODULES), so
 * context, reactivity and kit styles are one copy. The host mounts `component`
 * in a window and provides the app API via context: read it with `useAppApi()`.
 */
import type { Component } from 'svelte'
import { getContext, setContext } from 'svelte'
import type { ToolInfo } from '../tools'

export interface AppClientApi {
  readonly appId: string
  /** Call a handler this app's server module registered with `ctx.handle(channel)`. */
  invoke<T = unknown>(channel: string, payload?: unknown): Promise<T>
  /** Listen for `ctx.emit(event)` from this app's server module. */
  on(event: string, listener: (payload: unknown) => void): () => void
  /** The universal tool registry — every app's tools. */
  tools: {
    list(): Promise<ToolInfo[]>
    call<T = unknown>(id: string, args?: Record<string, unknown>): Promise<T>
  }
  /** Host services. */
  host: {
    openApp(appId: string): void
  }
}

export interface AppClientModule {
  component: Component
}

export function defineClientApp(module: AppClientModule): AppClientModule {
  return module
}

const KEY = Symbol.for('agentos.app-api')

/** Host-only: provide the API to an app's component tree. */
export function provideAppApi(api: AppClientApi): void {
  setContext(KEY, api)
}

/** Read the app API inside an app component. */
export function useAppApi(): AppClientApi {
  const api = getContext<AppClientApi | undefined>(KEY)
  if (!api) throw new Error('useAppApi() called outside an AgentOS app window')
  return api
}
