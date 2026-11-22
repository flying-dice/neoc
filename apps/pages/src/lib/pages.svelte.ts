import type { Api, Engine, Host, Logs, Overview, Server, Settings, Site } from 'app:rpc'
import { hostnamesOf, portOf } from './format'

/** What the window needs of the backend: the overview, each way of changing it, and the server's log. */
export type PagesRpc = Pick<
  Api,
  'overview' | 'connect' | 'apply' | 'saveSettings' | 'addSite' | 'updateSite' | 'setSiteEnabled' | 'removeSite' | 'logs'
>
/** What the window needs of the host: the backend's events (`host.on`), and a site opened outside the app. */
export type PagesHost = Pick<Host, 'on' | 'openLink'>

/** What of the server is looked at. */
export type Tab = 'sites' | 'settings' | 'log'

/** A site as it is typed, before it is saved: the port and the hostnames are text until then. */
export interface SiteDraft {
  name: string
  folder: string
  port: string
  hostnames: string
}

/** The site that is being typed: a new one, or one of the list being changed. */
export interface Editing {
  /** The id of the site being changed; nothing for a new one. */
  site: number | null
  draft: SiteDraft
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

const NOTHING: Overview = {
  engine: { reachable: false, version: '', host: '', problem: '' },
  server: { wanted: false, state: 'absent', status: '', current: false, image: '', problem: '' },
  settings: { port: 80 },
  sites: []
}

const NO_SITE: SiteDraft = { name: '', folder: '', port: '', hostnames: '' }

const draftOf = (site: Site): SiteDraft => ({
  name: site.name,
  folder: site.folder,
  port: site.port > 0 ? String(site.port) : '',
  hostnames: site.hostnames.join(', ')
})

/**
 * One window's Pages. The server, its settings and the sites are the backend's: every change is a call that answers
 * with the whole overview, and this holds the last one. What is the window's own is here: what is typed and not yet
 * sent, which tab is in front, and what is in flight.
 */
export class Pages {
  // `raw`: the overview is replaced whole by each answer, never edited in place
  #overview = $state.raw<Overview>(NOTHING)
  // nothing has been read yet: the window says so instead of "Docker is not running"
  #loaded = $state(false)
  // why the last call failed: the backend's own message, or the host's. It stays until a call works
  #failure = $state<string | null>(null)
  #tab = $state<Tab>('sites')
  // the container is being made, started or removed: it can take as long as pulling the image does
  #working = $state(false)
  // the ids of the sites a call is in flight for
  #busy = $state.raw<ReadonlySet<number>>(new Set())
  #editing = $state.raw<Editing | null>(null)
  // what is typed for the shared port; nothing while it is as it is kept
  #port = $state<string | null>(null)
  #logs = $state.raw<Logs | null>(null)
  /** What is typed as the address of another Docker. */
  hostDraft = $state('')

  readonly #rpc: PagesRpc
  readonly #host: PagesHost

  constructor(rpc: PagesRpc, host: PagesHost) {
    this.#rpc = rpc
    this.#host = host
  }

  get engine(): Engine {
    return this.#overview.engine
  }
  get server(): Server {
    return this.#overview.server
  }
  /** The settings as they are kept. */
  get settings(): Settings {
    return this.#overview.settings
  }
  get sites(): readonly Site[] {
    return this.#overview.sites
  }
  /** The first answer has arrived. */
  get loaded(): boolean {
    return this.#loaded
  }
  get failure(): string | null {
    return this.#failure
  }
  get tab(): Tab {
    return this.#tab
  }
  /** The container is being made, started or removed. */
  get working(): boolean {
    return this.#working
  }
  /** The site that is being typed; nothing while the list is shown. */
  get editing(): Editing | null {
    return this.#editing
  }
  /** The server's log, once it is read; nothing while the server has no container. */
  get logs(): Logs | null {
    return this.#logs
  }

  /** The shared port as it is typed: what is kept, until something is. */
  readonly portDraft: string = $derived(this.#port ?? String(this.#overview.settings.port))
  /** The shared port is typed and not saved. */
  readonly settingsChanged: boolean = $derived(this.#port !== null && portOf(this.#port) !== this.#overview.settings.port)
  /** New lines may arrive for what is shown: the log of a server that runs. */
  readonly following: boolean = $derived(this.#tab === 'log' && this.#overview.server.state === 'running')

  /** A call is in flight for this site. */
  busy(site: Site): boolean {
    return this.#busy.has(site.id)
  }

  /** Take an answer of the backend as how things are now. */
  #take(overview: Overview): void {
    this.#overview = overview
    // the address field starts as what is chosen
    if (!this.#loaded) this.hostDraft = overview.engine.host
    this.#loaded = true
  }

  /** Run a call that answers with the overview. It never rejects: a failure is kept, to be shown. */
  async #show(call: () => Promise<Overview>): Promise<boolean> {
    try {
      this.#take(await call())
      this.#failure = null
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** The same, for a call that may make the container again: the window says it is working. */
  async #change(call: () => Promise<Overview>): Promise<boolean> {
    this.#working = true
    try {
      return await this.#show(call)
    } finally {
      this.#working = false
    }
  }

  /** The same, for a call about one site: its row says so. */
  async #changeSite(site: Site, call: () => Promise<Overview>): Promise<boolean> {
    this.#busy = new Set([...this.#busy, site.id])
    try {
      return await this.#change(call)
    } finally {
      this.#busy = new Set([...this.#busy].filter((id) => id !== site.id))
    }
  }

  /** Read how things are. */
  async load(): Promise<void> {
    await this.#show(() => this.#rpc.overview())
  }

  /**
   * Hear the backend while the window is open: the container changed (it crashed, it was stopped in a terminal), the
   * sites did (another window, an agent), or the host connection came back (a change made while it was away
   * reached nobody). Returns a function that stops listening.
   */
  listen(): () => void {
    const again = (): void => {
      void this.load()
      void this.refreshLog()
    }
    const stops = [this.#host.on('changed', again), this.#host.on('host:reconnected', again)]
    return () => stops.forEach((stop) => stop())
  }

  /** Make the container again and start it: the way out when the server is not running. */
  apply(): Promise<boolean> {
    return this.#change(() => this.#rpc.apply())
  }

  /** Open one of a site's addresses outside the app. A host that will not is said, like any failure. */
  async open(url: string): Promise<void> {
    try {
      await this.#host.openLink(url)
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Start typing a new site. */
  startAdding(): void {
    this.#editing = { site: null, draft: NO_SITE }
  }

  /** Start changing a site of the list. */
  startEditing(site: Site): void {
    this.#editing = { site: site.id, draft: draftOf(site) }
  }

  /** Type something of the site that is being typed. */
  editSite(change: Partial<SiteDraft>): void {
    if (this.#editing) this.#editing = { ...this.#editing, draft: { ...this.#editing.draft, ...change } }
  }

  /** Forget what is typed of the site, and show the list again. */
  cancelEditing(): void {
    this.#editing = null
  }

  /** Save the site that is typed; the list is shown again once it is. A port that is none is said here: the backend is not asked. */
  async saveSite(): Promise<boolean> {
    const editing = this.#editing
    if (!editing) return false
    const { name, folder, port: typed, hostnames } = editing.draft
    const port = typed.trim() === '' ? 0 : portOf(typed)
    if (port === undefined) {
      this.#failure = 'The port is a number from 1 to 65535, or empty for a site with only hostnames.'
      return false
    }
    const given = { name: name.trim(), folder: folder.trim(), port, hostnames: hostnamesOf(hostnames) }
    const id = editing.site
    const saved = await this.#change(() => (id === null ? this.#rpc.addSite(given) : this.#rpc.updateSite({ site: id, ...given })))
    if (saved) this.#editing = null
    return saved
  }

  /** Turn a site on or off. */
  setSiteEnabled(site: Site, enabled: boolean): Promise<boolean> {
    return this.#changeSite(site, () => this.#rpc.setSiteEnabled({ site: site.id, enabled }))
  }

  /** Stop hosting a site and forget it. */
  removeSite(site: Site): Promise<boolean> {
    return this.#changeSite(site, () => this.#rpc.removeSite({ site: site.id }))
  }

  /** Type the shared port. */
  editPort(typed: string): void {
    this.#port = typed
  }

  /** Forget what is typed of the settings. */
  resetSettings(): void {
    this.#port = null
  }

  /** Save the shared port that is typed. A port that is none is said here: the backend is not asked. */
  async saveSettings(): Promise<boolean> {
    const port = portOf(this.portDraft)
    if (port === undefined) {
      this.#failure = 'The port is a number from 1 to 65535.'
      return false
    }
    const saved = await this.#change(() => this.#rpc.saveSettings({ port }))
    if (saved) this.#port = null
    return saved
  }

  /** Run the server in the Docker at the address that is typed; empty is this machine's own. */
  async connect(): Promise<void> {
    if (await this.#show(() => this.#rpc.connect({ host: this.hostDraft.trim() }))) this.hostDraft = this.engine.host
  }

  /** Look at the sites, the settings or the log. */
  async show(tab: Tab): Promise<void> {
    this.#tab = tab
    await this.refreshLog()
  }

  /** Read the log again while it is what is shown. A server with no container has none: that is not a failure. */
  async refreshLog(): Promise<void> {
    if (this.#tab !== 'log') return
    if (this.server.state === 'absent') {
      this.#logs = null
      return
    }
    try {
      const logs = await this.#rpc.logs()
      if (this.#tab === 'log') this.#logs = logs
    } catch (e) {
      if (this.#tab === 'log') this.#failure = messageOf(e)
    }
  }
}
