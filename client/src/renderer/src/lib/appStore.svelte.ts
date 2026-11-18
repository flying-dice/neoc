import { hostBridge, type KnownApp } from '@agentos/sdk/host/client'

export type ListingState = 'get' | 'open' | 'update'

/**
 * The app store: the apps the server knows how to install, and install / update /
 * uninstall. Refreshes whenever the installed set changes (from any client)
 * and after reconnecting.
 */
class AppStore {
  known = $state<KnownApp[]>([])
  /** App ids with an install/uninstall in flight. */
  busy = $state<Record<string, boolean>>({})
  error = $state<string | null>(null)

  constructor() {
    const host = hostBridge()
    if (!host) return
    void this.refresh()
    host.apps.onChanged(() => void this.refresh())
    host.connection.onStatus((s) => s === 'open' && void this.refresh())
  }

  stateOf(app: KnownApp): ListingState {
    if (!app.installedVersion) return 'get'
    return app.installedVersion === app.manifest.version ? 'open' : 'update'
  }

  async refresh(): Promise<void> {
    try {
      this.known = (await hostBridge()?.apps.known()) ?? []
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e)
    }
  }

  async install(app: KnownApp): Promise<boolean> {
    return this.#run(app.manifest.id, () => hostBridge()!.apps.install(app.manifest.id))
  }

  async uninstall(app: KnownApp): Promise<boolean> {
    return this.#run(app.manifest.id, () => hostBridge()!.apps.uninstall(app.manifest.id))
  }

  async #run(id: string, op: () => Promise<unknown>): Promise<boolean> {
    if (this.busy[id] || !hostBridge()) return false
    this.busy[id] = true
    this.error = null
    try {
      await op()
      await this.refresh()
      return true
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e)
      return false
    } finally {
      delete this.busy[id]
    }
  }
}

export const appStore = new AppStore()
