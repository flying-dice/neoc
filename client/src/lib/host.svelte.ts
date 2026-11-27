import { Session } from '../shell/session'
import type { Transport } from '../shell/rpc/transport'
import { createWebSocketTransport, defaultRpcUrl } from '../shell/rpc/websocket'
import { messageOf } from '../shell/errors'
import { createHostChannel, HostError, type CallOptions, type HostChannel } from '../shell/channel'
import type { AppSummary, Catalog, HostApi } from '../shell/hostApi'
import { isPaired, pairDevice, type PairResult } from '../shell/pairing'
import { appListSchema, callParsed, catalogSchema } from '../shell/results'
import { setInstalledApps } from './apps.svelte'

/**
 * Where the shell stands with the host:
 *
 *   checking → unpaired → (pair) → connecting → ready          the normal first run
 *   checking → connecting → ready                              a paired device
 *   checking → unreachable                                     the host did not answer
 *
 * `ready` means the session is online at least once; a later drop is `connection`, which the status bar shows.
 */
export type Phase = 'checking' | 'unpaired' | 'connecting' | 'ready' | 'unreachable'

/** The host asks for an app's window (`window.open`). */
export interface WindowRequest {
  app: string
}

/** The app lifecycle notifications that make the app list stale. */
const LIST_CHANGED = new Set([
  'app.installed',
  'app.uninstalled',
  'app.enabled',
  'app.disabled',
  'app.updated',
  'app.failed'
])

/** What the host connection needs from the outside world: swapped for fakes in tests. */
export interface HostDeps {
  isPaired(): Promise<boolean>
  pairDevice(code: string, deviceName: string): Promise<PairResult>
  createTransport(): Transport
  /** Run `run` once, `ms` from now. */
  schedule(run: () => void, ms: number): void
}

const browserDeps: HostDeps = {
  isPaired: () => isPaired(),
  pairDevice: (code, deviceName) => pairDevice(code, deviceName),
  createTransport: () => createWebSocketTransport({ url: defaultRpcUrl(location) }),
  schedule: (run, ms) => void setTimeout(run, ms)
}

/** A socket that never opens may be a revoked session (the host answers 401): ask once, after this long. */
const REVOKED_CHECK_MS = 3000

export class Host {
  phase = $state<Phase>('checking')
  /** Why `unreachable`, or the last error worth showing. */
  error = $state<string | null>(null)
  connection = $state<'connecting' | 'online' | 'offline' | 'incompatible'>('connecting')
  /** Every installed app, whatever its state (the Dock shows the running ones). */
  apps = $state<AppSummary[]>([])
  /** The last `app.list` failure (a host that is down, or answers in a shape the shell does not know). */
  appsError = $state<string | null>(null)
  session: Session | null = null
  private channel: HostChannel | null = null
  private fake: HostApi | null = null
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- a list of callbacks, not state
  private windowRequests = new Set<(request: WindowRequest) => void>()
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- a list of callbacks, not state
  private appNotificationListeners = new Set<(params: unknown) => void>()
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- a list of callbacks, not state
  private uninstalledListeners = new Set<(app: string) => void>()
  private fakeNotifications: (() => void) | null = null
  private refreshing: Promise<void> | null = null
  private stale = false

  constructor(private readonly deps: HostDeps = browserDeps) {}

  /** The host-management calls (`app.*`, `host.*`): the session's connection, or a fake in stories. */
  readonly api: HostApi = {
    call: (
      method: string,
      params: Record<string, unknown> = {},
      options?: CallOptions
    ): Promise<unknown> => {
      if (this.fake) return this.fake.call(method, params)
      if (!this.channel) return Promise.reject(new HostError(-32000, 'Not connected to vos'))
      return this.channel.call(method, params, options)
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
      if (await this.deps.isPaired()) this.connect()
      else this.phase = 'unpaired'
    } catch (e) {
      this.error = messageOf(e)
      this.phase = 'unreachable'
    }
  }

  /** Trade a pairing code for a session, then connect. */
  async pair(code: string, deviceName: string): Promise<PairResult> {
    const result = await this.deps.pairDevice(code, deviceName)
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
    const channel = createHostChannel(this.deps.createTransport())
    this.channel = channel
    const session = new Session({
      transport: channel.transport,
      onError: (e) => console.warn('[vos]', e)
    })
    this.session = session
    session.onState((state) => {
      this.connection = state
      if (state === 'online') {
        this.phase = 'ready'
        void this.refresh()
      } else if (state === 'offline') void this.verifyPairing()
    })
    channel.onNotification((method, params) => this.onNotification(method, params))
    session.start()
    this.deps.schedule(() => {
      if (this.session === session && !session.online) void this.verifyPairing()
    }, REVOKED_CHECK_MS)
  }

  /** After a drop: is this device still paired? A revoked one goes back to the pairing screen. */
  private async verifyPairing(): Promise<void> {
    try {
      if (await this.deps.isPaired()) return
    } catch {
      return // the host is just not there: keep reconnecting
    }
    this.session?.stop()
    this.session = null
    this.channel = null
    this.setApps([]) // the registry and the Dock roster too: nothing of the revoked session stays on screen
    this.phase = 'unpaired'
  }

  private onNotification(method: string, params: Record<string, unknown>): void {
    if (method === 'window.open' && typeof params.app === 'string') {
      const request: WindowRequest = { app: params.app }
      for (const l of [...this.windowRequests]) l(request)
    } else if (method === 'notification.deliver') {
      for (const l of [...this.appNotificationListeners]) l(params)
    } else if (LIST_CHANGED.has(method)) {
      // told before the list is read again: an app uninstalled and installed again under the same id can look, in the
      // refreshed list, as if it never left
      if (method === 'app.uninstalled' && typeof params.app === 'string') {
        for (const l of [...this.uninstalledListeners]) l(params.app)
      }
      void this.refresh()
    }
  }

  /** The host delivers an app's notification (`notification.deliver`); the shell decides whether to show it. */
  onAppNotification(listener: (params: unknown) => void): () => void {
    this.appNotificationListeners.add(listener)
    return () => void this.appNotificationListeners.delete(listener)
  }

  /** An app was uninstalled (`app.uninstalled`): whatever the shell holds of that instance of it goes with it. */
  onAppUninstalled(listener: (app: string) => void): () => void {
    this.uninstalledListeners.add(listener)
    return () => void this.uninstalledListeners.delete(listener)
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
          const { apps } = await callParsed(this.api, 'app.list', {}, appListSchema)
          this.setApps(apps)
          this.appsError = null
        } catch (e) {
          this.appsError = messageOf(e)
          console.warn('[vos] app.list:', e)
        }
      } while (this.stale)
    })().finally(() => (this.refreshing = null))
    return this.refreshing
  }

  private setApps(apps: AppSummary[]): void {
    this.apps = apps
    setInstalledApps(apps)
  }

  /** What the configured registries offer (Discover). */
  catalog(): Promise<Catalog> {
    return callParsed(this.api, 'app.available', {}, catalogSchema)
  }

  /** Stories and tests: serve the shell from a fake instead of a host. */
  useFake(api: HostApi, apps: AppSummary[] = []): void {
    this.fake = api
    this.fakeNotifications?.()
    // a story's fake can deliver an app's notification (the other messages are for the pages that listen to the fake)
    this.fakeNotifications = api.onNotification((method, params) => {
      if (method === 'notification.deliver' || method === 'app.uninstalled')
        this.onNotification(method, params)
    })
    this.phase = 'ready'
    this.connection = 'online'
    this.setApps(apps)
  }
}

export const host = new Host()
