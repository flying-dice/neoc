import { app } from 'electron'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { AppMainContext } from '@agentos/sdk/app/main'
import type { HostEmitter } from '@agentos/sdk/host/main'
import type { InstalledApp } from './installedApps'
import type { ToolRegistry } from './toolRegistry'

type Handler = (payload: unknown) => unknown | Promise<unknown>

/**
 * Loads app main modules and gives each one a sandboxed context: namespaced
 * IPC handlers, tool registration, events to its own renderer, and private
 * storage. Apps never see Electron or each other.
 */
export class AppHost {
  #handlers = new Map<string, Map<string, Handler>>()

  constructor(
    private readonly tools: ToolRegistry,
    private readonly emit: HostEmitter
  ) {}

  async load(apps: InstalledApp[]): Promise<void> {
    for (const installed of apps) {
      const { id } = installed.manifest
      try {
        await installed.main.setup(this.#context(id))
        console.log(`[agentos] app "${id}" loaded`)
      } catch (error) {
        console.error(`[agentos] app "${id}" failed to load:`, error)
      }
    }
  }

  /** Dispatch a renderer `api.invoke` to the app's registered handler. */
  invoke(appId: string, channel: string, payload: unknown): Promise<unknown> {
    const handler = this.#handlers.get(appId)?.get(channel)
    if (!handler) return Promise.reject(new Error(`App "${appId}" has no handler for "${channel}"`))
    return Promise.resolve(handler(payload))
  }

  #context(appId: string): AppMainContext {
    const handlers = new Map<string, Handler>()
    this.#handlers.set(appId, handlers)
    const storage = new AppStorage(appId)
    return {
      appId,
      handle: (channel, handler) => {
        handlers.set(channel, handler)
      },
      registerTool: (tool) => this.tools.register(appId, tool),
      emit: (event, payload) => this.emit('apps:event', { appId, event, payload }),
      openWindow: () => this.emit('apps:open', { appId }),
      storage: {
        get: (key) => storage.get(key),
        set: (key, value) => storage.set(key, value)
      },
      log: (...args) => console.log(`[${appId}]`, ...args)
    }
  }
}

/** Tiny per-app JSON store under userData/apps/<id>/storage.json. */
class AppStorage {
  #file: string
  #data: Record<string, unknown>

  constructor(appId: string) {
    const dir = join(app.getPath('userData'), 'apps', appId)
    mkdirSync(dir, { recursive: true })
    this.#file = join(dir, 'storage.json')
    try {
      this.#data = JSON.parse(readFileSync(this.#file, 'utf8')) as Record<string, unknown>
    } catch {
      this.#data = {}
    }
  }

  get<T>(key: string): T | undefined {
    return this.#data[key] as T | undefined
  }

  set(key: string, value: unknown): void {
    this.#data[key] = value
    writeFileSync(this.#file, JSON.stringify(this.#data, null, 2))
  }
}
