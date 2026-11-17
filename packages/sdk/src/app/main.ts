/**
 * Main-process side of an app module.
 *
 *   export default defineMainApp((ctx) => {
 *     ctx.handle('evaluate', (expr) => …)       // renderer → main calls
 *     ctx.registerTool({ name: 'calculate', … }) // agents + renderers
 *   })
 *
 * Apps never import Electron or touch other apps — everything goes through
 * the context the host provides.
 */
import type { z } from 'zod'
import type { AppToolDefinition } from '../tools'

export interface AppMainContext {
  readonly appId: string
  /** Handle `api.invoke(channel, payload)` calls from this app's renderer. */
  handle(channel: string, handler: (payload: unknown) => unknown | Promise<unknown>): void
  /** Expose a tool to agents (via MCP) and to app renderers. */
  registerTool<Shape extends z.ZodRawShape>(tool: AppToolDefinition<Shape>): void
  /** Push an event to this app's renderer (`api.on(event, cb)`). */
  emit(event: string, payload?: unknown): void
  /** Ask the host to open (or focus) this app's window. */
  openWindow(): void
  /** Small persistent key/value store, private to this app. */
  storage: {
    get<T>(key: string): T | undefined
    set(key: string, value: unknown): void
  }
  log(...args: unknown[]): void
}

export type AppMainSetup = (ctx: AppMainContext) => void | Promise<void>

export interface AppMainModule {
  setup: AppMainSetup
}

export function defineMainApp(setup: AppMainSetup): AppMainModule {
  return { setup }
}
