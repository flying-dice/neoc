import {
  reactiveStore,
  Session,
  createWebSocketTransport,
  defaultRpcUrl,
  type ViewCore
} from '../view'
import { createHostChannel, HostError, type HostChannel } from '../shell/channel'
import type { AgentRow, AppSummary, Catalog, HostApi } from '../shell/hostApi'
import { isPaired, pairDevice, type PairResult } from '../shell/pairing'
import { setInstalledApps } from './apps.svelte'

/**
 * Where the shell stands with the host:
 *
 *   checking → unpaired → (pair) → connecting → ready          the normal first run
 *   checking → connecting → ready                              a paired device
 *   checking → unreachable                                     the host did not answer
 *
 * `ready` means the session is online at least once; a later drop is `connection`, which the status bar shows
 * while every open window dims and keeps its drafts (the renderer's offline mode).
 */
export type Phase = 'checking' | 'unpaired' | 'connecting' | 'ready' | 'unreachable'

export interface WindowRequest {
  app: string
  window: string
  params?: Record<string, unknown>
}

/** The app lifecycle notifications that make the app list stale. */
const LIST_CHANGED = new Set([
  'app.installed',
  'app.uninstalled',
  'app.enabled',
  'app.disabled',
  'app.failed'
])

class Host {
  phase = $state<Phase>('checking')
  /** Why `unreachable`, or the last error worth showing. */
  error = $state<string | null>(null)
  connection = $state<'connecting' | 'online' | 'offline' | 'incompatible'>('connecting')
  /** Every installed app, whatever its state (the Dock shows the running ones). */
  apps = $state<AppSummary[]>([])
  /** The agents on the host's roster (the desktop widget). */
  agents = $state<AgentRow[]>([])
  session: Session | null = null
  private channel: HostChannel | null = null
  private fake: HostApi | null = null
  private windowRequests = new Set<(request: WindowRequest) => void>()
  private refreshing: Promise<void> | null = null
  private stale = false

  /** The host-management calls (`app.*`, `host.*`): the session's connection, or a fake in stories. */
  readonly api: HostApi = {
    call: <T>(method: string, params: Record<string, unknown> = {}): Promise<T> => {
      if (this.fake) return this.fake.call<T>(method, params)
      if (!this.channel) return Promise.reject(new HostError(-32000, 'Not connected to AgentOS'))
      return this.channel.call<T>(method, params)
    },
    onNotification: (listener) =>
      this.fake
        ? this.fake.onNotification(listener)
        : (this.channel?.onNotification(listener) ?? (() => {}))
  }

  /** First thing at startup: ask whether this browser is paired, then connect. */
  async start(): Promise<void> {
    this.phase = 'checking'
    try {
      if (await isPaired()) this.connect()
      else this.phase = 'unpaired'
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e)
      this.phase = 'unreachable'
    }
  }

  /** Trade a pairing code for a session, then connect. */
  async pair(code: string, deviceName: string): Promise<PairResult> {
    const result = await pairDevice(code, deviceName)
    if (result.ok) this.connect()
    return result
  }

  /** The page was served by a host the shell cannot reach right now (offline PWA, host stopped). */
  retry(): void {
    if (this.phase === 'unreachable') void this.start()
    else this.session?.retryNow()
  }

  private connect(): void {
    this.phase = 'connecting'
    const channel = createHostChannel(createWebSocketTransport({ url: defaultRpcUrl(location) }))
    this.channel = channel
    const session = new Session({
      transport: channel.transport,
      createStore: reactiveStore,
      onError: (e) => console.warn('[agentos]', e)
    })
    this.session = session
    session.onState((state) => {
      this.connection = state
      if (state === 'online') {
        this.phase = 'ready'
        void this.refresh()
        void this.refreshAgents()
      } else if (state === 'offline') void this.verifyPairing()
    })
    channel.onNotification((method, params) => this.onNotification(method, params))
    session.start()
    // a socket that never opens may be a revoked session (the host answers 401): ask once
    setTimeout(() => {
      if (this.session === session && !session.online) void this.verifyPairing()
    }, 3000)
  }

  /** After a drop: is this device still paired? A revoked one goes back to the pairing screen. */
  private async verifyPairing(): Promise<void> {
    try {
      if (await isPaired()) return
    } catch {
      return // the host is just not there: keep reconnecting
    }
    this.session?.stop()
    this.session = null
    this.channel = null
    this.apps = []
    this.phase = 'unpaired'
  }

  private onNotification(method: string, params: Record<string, unknown>): void {
    if (method === 'window.open' && typeof params.app === 'string') {
      const request: WindowRequest = {
        app: params.app,
        window: typeof params.window === 'string' ? params.window : 'main',
        params: params.params as Record<string, unknown> | undefined
      }
      for (const l of [...this.windowRequests]) l(request)
    } else if (LIST_CHANGED.has(method)) void this.refresh()
    else if (method === 'agents.changed') void this.refreshAgents()
  }

  /** The shell opens a window when an app asks (an agent's tool bringing it forward). */
  onWindowRequest(listener: (request: WindowRequest) => void): () => void {
    this.windowRequests.add(listener)
    return () => void this.windowRequests.delete(listener)
  }

  /** Re-read the installed apps. Calls made while one is running collapse into one more run. */
  refresh(): Promise<void> {
    if (this.refreshing) {
      this.stale = true
      return this.refreshing
    }
    this.refreshing = (async () => {
      do {
        this.stale = false
        try {
          const { apps } = await this.api.call<{ apps: AppSummary[] }>('app.list')
          this.setApps(apps)
        } catch (e) {
          console.warn('[agentos] app.list:', e)
        }
      } while (this.stale)
    })().finally(() => (this.refreshing = null))
    return this.refreshing
  }

  private async refreshAgents(): Promise<void> {
    try {
      this.agents = (await this.api.call<{ agents: AgentRow[] }>('agents.list')).agents
    } catch (e) {
      console.warn('[agentos] agents.list:', e)
    }
  }

  private setApps(apps: AppSummary[]): void {
    this.apps = apps
    setInstalledApps(apps)
  }

  /** What the configured registries offer (Discover). */
  catalog(): Promise<Catalog> {
    return this.api.call<Catalog>('app.available')
  }

  openView(...args: Parameters<Session['openView']>): Promise<ViewCore> {
    if (!this.session) return Promise.reject(new Error('Not connected to AgentOS'))
    return this.session.openView(...args)
  }

  closeView(view: ViewCore): Promise<void> {
    return this.session?.closeView(view) ?? Promise.resolve()
  }

  /** Stories and tests: serve the shell from a fake instead of a host. */
  useFake(api: HostApi, apps: AppSummary[] = []): void {
    this.fake = api
    this.phase = 'ready'
    this.connection = 'online'
    this.setApps(apps)
    void this.refreshAgents()
  }
}

export const host = new Host()
