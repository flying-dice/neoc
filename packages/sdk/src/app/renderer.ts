/**
 * Renderer side of an app module.
 *
 *   export default defineRendererApp({ component: CalculatorPage })
 *
 * The host mounts `component` inside a window and provides the app API via
 * Svelte context; components read it with `useAppApi()`.
 */
import type { Component } from 'svelte'
import { getContext, setContext } from 'svelte'
import type { ToolInfo } from '../tools'

export interface AppRendererApi {
  readonly appId: string
  /** Call a handler this app registered in main with `ctx.handle(channel)`. */
  invoke<T = unknown>(channel: string, payload?: unknown): Promise<T>
  /** Listen for `ctx.emit(event)` from this app's main module. */
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

export interface AppRendererModule {
  component: Component
}

export function defineRendererApp(module: AppRendererModule): AppRendererModule {
  return module
}

const KEY = Symbol.for('agentos.app-api')

/** Host-only: provide the API to an app's component tree. */
export function provideAppApi(api: AppRendererApi): void {
  setContext(KEY, api)
}

/** Read the app API inside an app component. */
export function useAppApi(): AppRendererApi {
  const api = getContext<AppRendererApi | undefined>(KEY)
  if (!api) throw new Error('useAppApi() called outside an AgentOS app window')
  return api
}
