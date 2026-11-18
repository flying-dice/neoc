/**
 * Server side of an app module: runs in the AgentOS Bun server, built as a
 * standalone ESM bundle (`server.js`) and registered / unregistered at runtime.
 *
 *   export default defineServerApp((ctx) => {
 *     const db = new Database(join(ctx.dataDir!, 'app.sqlite'))   // bun:sqlite, any Bun API
 *     ctx.handle('evaluate', (expr) => …)       // client → server calls
 *     ctx.registerTool({ name: 'calculate', … }) // agents (MCP) + clients
 *     return () => db.close()                    // runs on unregister
 *   })
 *
 * Lifecycle: the host calls `setup` when the app is installed or the server
 * starts, and unregisters it on uninstall, update or shutdown: it aborts
 * `ctx.signal`, awaits the returned dispose function, then drops the app's
 * handlers and tools. Release everything you opened (databases, timers,
 * watchers, sockets) there, or the old version keeps running beside the new.
 *
 * Apps never touch the host's internals or other apps: everything goes through
 * this context. Talk to other apps through their tools.
 */
import type { z } from 'zod'
import type { AppToolDefinition } from '../tools'

export interface AppServerContext {
  readonly appId: string
  readonly version: string
  /** Aborted when the app is unregistered (uninstall, update, server shutdown). */
  readonly signal: AbortSignal
  /**
   * A directory owned by this app for its own files (e.g. a SQLite database).
   * It survives updates. Undefined where there is no filesystem (the in-memory
   * test host): fall back to `storage` or provide a `src/mock/server.ts`.
   */
  readonly dataDir: string | undefined
  /** Handle `api.invoke(channel, payload)` calls from this app's client. */
  handle(channel: string, handler: (payload: unknown) => unknown | Promise<unknown>): void
  /** Expose a tool to agents (via MCP) and to app clients. */
  registerTool<Shape extends z.ZodRawShape>(tool: AppToolDefinition<Shape>): void
  /** Push an event to this app's open clients (`api.on(event, cb)`). */
  emit(event: string, payload?: unknown): void
  /** Ask connected clients to open (or focus) this app's window. */
  openWindow(): void
  /** Small persistent key/value store, private to this app (JSON values). */
  storage: {
    get<T>(key: string): T | undefined
    set(key: string, value: unknown): void
  }
  log(...args: unknown[]): void
}

/** Returned from setup to release resources when the app is unregistered. */
export type AppDispose = () => void | Promise<void>

export type AppServerSetup = (ctx: AppServerContext) => void | AppDispose | Promise<void | AppDispose>

export interface AppServerModule {
  setup: AppServerSetup
}

export function defineServerApp(setup: AppServerSetup): AppServerModule {
  return { setup }
}
