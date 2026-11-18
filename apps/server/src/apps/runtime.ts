import { mkdir, rm } from 'node:fs/promises'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import type { AppManifest } from '@agentos/sdk'
import type { AppDispose, AppServerContext, AppServerModule } from '@agentos/sdk/app/server'
import type { InstalledApp } from '@agentos/sdk/host/protocol'
import type { HostEmitter } from '@agentos/sdk/host/server'
import type { InstalledRow, SystemDb } from '../db'
import type { ToolRegistry } from '../tools/registry'

type Handler = (payload: unknown) => unknown | Promise<unknown>

interface Running {
  row: InstalledRow
  status: 'running' | 'failed'
  error?: string
  controller: AbortController
  handlers: Map<string, Handler>
  dispose?: AppDispose | void
}

const isServerModule = (value: unknown): value is AppServerModule =>
  typeof value === 'object' && value !== null && typeof (value as AppServerModule).setup === 'function'

/**
 * Registers and unregisters app server modules at runtime.
 *
 *  • Bundles live in `<data>/bundles/<id>/<version>-<sha>/`: a rebuilt bundle
 *    gets a new path, so `import()` never serves a stale module from cache.
 *  • Each app gets its own context: namespaced handlers, tools, events, kv
 *    storage, a data directory and an AbortSignal for its lifetime.
 *  • Failures are isolated: a setup that throws marks that app "failed" and
 *    leaves the server and other apps running.
 */
export class AppRuntime {
  readonly #apps = new Map<string, Running>()

  constructor(
    private readonly db: SystemDb,
    private readonly tools: ToolRegistry,
    private readonly emit: HostEmitter,
    private readonly dataDir: string
  ) {}

  bundleDir(id: string, version: string, serverSha: string): string {
    return join(this.dataDir, 'bundles', id, `${version}-${serverSha.slice(0, 12)}`)
  }

  /** Register every app recorded as installed (server start). */
  async start(): Promise<void> {
    for (const row of this.db.installed()) await this.#register(row)
  }

  /** Write verified bundles, record the install and (re)register the app. */
  async install(
    registry: string,
    manifest: AppManifest,
    bundles: { server: Uint8Array; serverSha: string; client: Uint8Array; clientSha: string }
  ): Promise<InstalledApp> {
    const previous = this.#apps.get(manifest.id)?.row
    const dir = this.bundleDir(manifest.id, manifest.version, bundles.serverSha)
    await mkdir(dir, { recursive: true })
    await Bun.write(join(dir, 'server.js'), bundles.server)
    await Bun.write(join(dir, 'client.js'), bundles.client)
    await Bun.write(join(dir, 'manifest.json'), JSON.stringify(manifest, null, 2))

    const row: InstalledRow = {
      id: manifest.id,
      version: manifest.version,
      registry,
      manifest,
      serverSha: bundles.serverSha,
      clientSha: bundles.clientSha,
      installedAt: Date.now()
    }
    await this.unregister(manifest.id)
    this.db.saveInstalled(row)
    await this.#register(row)
    if (previous && this.bundleDir(previous.id, previous.version, previous.serverSha) !== dir) {
      await rm(this.bundleDir(previous.id, previous.version, previous.serverSha), { recursive: true, force: true })
    }
    this.#changed()
    return this.#toInstalled(this.#apps.get(manifest.id)!)
  }

  /** Unregister, forget and delete the app's bundles. Its data directory is kept. */
  async uninstall(id: string): Promise<void> {
    const running = this.#apps.get(id)
    if (!running) throw new Error(`App "${id}" is not installed`)
    await this.unregister(id)
    this.db.removeInstalled(id)
    await rm(join(this.dataDir, 'bundles', id), { recursive: true, force: true })
    this.#changed()
  }

  /** Stop an app: abort its signal, await its dispose, drop handlers and tools. */
  async unregister(id: string): Promise<void> {
    const running = this.#apps.get(id)
    if (!running) return
    this.#apps.delete(id)
    running.controller.abort()
    try {
      await running.dispose?.()
    } catch (error) {
      console.error(`[agentos] app "${id}" dispose failed:`, error)
    }
    this.tools.unregisterApp(id)
  }

  async stopAll(): Promise<void> {
    for (const id of [...this.#apps.keys()]) await this.unregister(id)
  }

  list(): InstalledApp[] {
    return [...this.#apps.values()].map((r) => this.#toInstalled(r))
  }

  installedRow(id: string): InstalledRow | undefined {
    return this.#apps.get(id)?.row
  }

  /** The client bundle for an installed app, or undefined (only installed bundles are served). */
  clientBundle(id: string, version: string): string | undefined {
    const row = this.#apps.get(id)?.row
    if (!row || row.version !== version) return undefined
    return join(this.bundleDir(row.id, row.version, row.serverSha), 'client.js')
  }

  invoke(appId: string, channel: string, payload: unknown): Promise<unknown> {
    const running = this.#apps.get(appId)
    if (!running || running.status !== 'running') return Promise.reject(new Error(`App "${appId}" is not running`))
    const handler = running.handlers.get(channel)
    if (!handler) return Promise.reject(new Error(`App "${appId}" has no handler for "${channel}"`))
    return Promise.resolve(handler(payload))
  }

  async #register(row: InstalledRow): Promise<void> {
    const running: Running = { row, status: 'running', controller: new AbortController(), handlers: new Map() }
    this.#apps.set(row.id, running)
    try {
      const file = join(this.bundleDir(row.id, row.version, row.serverSha), 'server.js')
      const mod = (await import(pathToFileURL(file).href)) as { default?: unknown }
      if (!isServerModule(mod.default)) throw new Error('server.js must default-export defineServerApp(...)')
      const appData = join(this.dataDir, 'apps', row.id)
      await mkdir(appData, { recursive: true })
      running.dispose = await mod.default.setup(this.#context(running, appData))
      console.log(`[agentos] app "${row.id}" ${row.version} registered`)
    } catch (error) {
      running.status = 'failed'
      running.error = error instanceof Error ? error.message : String(error)
      running.controller.abort()
      this.tools.unregisterApp(row.id)
      running.handlers.clear()
      console.error(`[agentos] app "${row.id}" failed to start:`, error)
    }
  }

  #context(running: Running, dataDir: string): AppServerContext {
    const { id, version } = running.row
    const live = (): boolean => this.#apps.get(id) === running && !running.controller.signal.aborted
    return {
      appId: id,
      version,
      signal: running.controller.signal,
      dataDir,
      handle: (channel, handler) => {
        running.handlers.set(channel, handler)
      },
      registerTool: (tool) => this.tools.register(id, tool),
      // A disposed app's stray timers must not reach clients.
      emit: (event, payload) => {
        if (live()) this.emit('apps:event', { appId: id, event, payload })
      },
      openWindow: () => {
        if (live()) this.emit('apps:open', { appId: id })
      },
      storage: {
        get: <T>(key: string) => this.db.kvGet(id, key) as T | undefined,
        set: (key, value) => this.db.kvSet(id, key, value)
      },
      log: (...args) => console.log(`[${id}]`, ...args)
    }
  }

  #toInstalled(r: Running): InstalledApp {
    return {
      manifest: r.row.manifest,
      clientUrl: `/bundles/${r.row.id}/${r.row.version}/client.js?v=${r.row.clientSha.slice(0, 12)}`,
      registry: r.row.registry,
      status: r.status,
      ...(r.error ? { error: r.error } : {})
    }
  }

  #changed(): void {
    this.emit('apps:changed', { apps: this.list() })
  }
}
