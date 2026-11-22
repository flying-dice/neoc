import type { Api, Engine, Host, Logs, Overview, Record as DnsRecord, Server, Settings } from 'app:rpc'
import { portOf, upstreamsOf } from './format'
import { addressProblem, nameProblem, portProblem, upstreamsProblem } from './validate'

/** What the window needs of the backend: the overview, each way of changing it, and the server's log. */
export type DnsRpc = Pick<Api, 'overview' | 'connect' | 'setEnabled' | 'apply' | 'saveSettings' | 'addRecord' | 'removeRecord' | 'logs'>
/** Where the backend's events are heard (`host.on`). */
export type DnsEvents = Pick<Host, 'on'>

/** What of the server is looked at. */
export type Tab = 'records' | 'settings' | 'log'

/** A record as it is typed, before it is added. */
export interface RecordDraft {
  name: string
  address: string
  subdomains: boolean
}

/** The settings as they are typed, before they are saved: the port and the upstream servers are text until then. */
export interface SettingsDraft {
  port: string
  upstreams: string
  logQueries: boolean
}

/** What is wrong with what is typed in each field of a form: a sentence, or nothing. */
export type Problems<Field extends string> = Readonly<Record<Field, string | null>>

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
/** The backend looked at what was sent and said no (`ctx:fail`): the message is about the input, not about the call. */
const isRefusal = (e: unknown): boolean => (e as { code?: unknown } | null)?.code === -32010

const NOTHING: Overview = {
  engine: { reachable: false, version: '', host: '', problem: '' },
  server: { enabled: false, state: 'absent', status: '', current: false, image: '', problem: '' },
  settings: { port: 53, upstreams: [], logQueries: false },
  records: []
}

const NO_RECORD: RecordDraft = { name: '', address: '', subdomains: false }

const draftOf = (settings: Settings): SettingsDraft => ({
  port: String(settings.port),
  upstreams: settings.upstreams.join(', '),
  logQueries: settings.logQueries
})

/**
 * One window's DNS. The server, its settings and the records are the backend's: every change is a call that answers
 * with the whole overview, and this holds the last one. What is the window's own is here: what is typed and not yet
 * sent, which tab is in front, and what is in flight.
 */
export class Dns {
  // `raw`: the overview is replaced whole by each answer, never edited in place
  #overview = $state.raw<Overview>(NOTHING)
  // nothing has been read yet: the window says so instead of "Docker is not running"
  #loaded = $state(false)
  // why the last call failed (the host's message, or Docker's): not what was typed, which is said at its field
  #failure = $state<string | null>(null)
  // what the backend refused of the record, and of the settings, as they were sent: it goes when they are typed again
  #recordRefused = $state<string | null>(null)
  #settingsRefused = $state<string | null>(null)
  #tab = $state<Tab>('records')
  // the container is being made, started or removed: it can take as long as pulling the image does
  #working = $state(false)
  // the ids of the records a call is in flight for
  #removing = $state.raw<ReadonlySet<number>>(new Set())
  #record = $state.raw<RecordDraft>(NO_RECORD)
  // what is typed for the settings; nothing while they are as they are kept
  #settings = $state.raw<SettingsDraft | null>(null)
  #logs = $state.raw<Logs | null>(null)
  /** What is typed as the address of another Docker. */
  hostDraft = $state('')

  readonly #rpc: DnsRpc
  readonly #events: DnsEvents

  constructor(rpc: DnsRpc, events: DnsEvents) {
    this.#rpc = rpc
    this.#events = events
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
  get records(): readonly DnsRecord[] {
    return this.#overview.records
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
  /** The record that is being typed. */
  get record(): RecordDraft {
    return this.#record
  }
  /** The server's log, once it is read; nothing while the server has no container. */
  get logs(): Logs | null {
    return this.#logs
  }

  /** The backend's word on the record that was sent and not added (the name has that address already). */
  get recordRefused(): string | null {
    return this.#recordRefused
  }
  /** The backend's word on the settings that were sent and not saved. */
  get settingsRefused(): string | null {
    return this.#settingsRefused
  }

  /** What is wrong with the record as it is typed, field by field. */
  readonly recordProblems: Problems<'name' | 'address'> = $derived({
    name: nameProblem(this.#record.name),
    address: addressProblem(this.#record.address)
  })
  /** What is wrong with the settings as they are typed, field by field. */
  readonly settingsProblems: Problems<'port' | 'upstreams'> = $derived.by(() => {
    const typed = this.#settings
    return typed === null ? { port: null, upstreams: null } : { port: portProblem(typed.port), upstreams: upstreamsProblem(typed.upstreams) }
  })

  /** The settings as they are typed: what is kept, until something is. */
  readonly settingsDraft: SettingsDraft = $derived(this.#settings ?? draftOf(this.#overview.settings))
  /** Something of the settings is typed and not saved. */
  readonly settingsChanged: boolean = $derived.by(() => {
    const typed = this.#settings
    if (typed === null) return false
    const kept = this.#overview.settings
    return (
      portOf(typed.port) !== kept.port || typed.logQueries !== kept.logQueries || upstreamsOf(typed.upstreams).join(' ') !== kept.upstreams.join(' ')
    )
  })
  /** New lines may arrive for what is shown: the log of a server that runs. */
  readonly following: boolean = $derived(this.#tab === 'log' && this.#overview.server.state === 'running')

  /** A call is in flight for this record. */
  removing(record: DnsRecord): boolean {
    return this.#removing.has(record.id)
  }

  /** Take an answer of the backend as how things are now. */
  #take(overview: Overview): void {
    this.#overview = overview
    // the address field starts as what is chosen
    if (!this.#loaded) this.hostDraft = overview.engine.host
    this.#loaded = true
  }

  /**
   * Run a call that answers with the overview. It never rejects: a failure is kept, to be shown. What the backend
   * refused of a form goes to `refused`, to be said at the form; everything else is the window's.
   */
  async #show(call: () => Promise<Overview>, refused?: (message: string) => void): Promise<boolean> {
    try {
      this.#take(await call())
      this.#failure = null
      return true
    } catch (e) {
      if (refused && isRefusal(e)) refused(messageOf(e))
      else this.#failure = messageOf(e)
      return false
    }
  }

  /** The same, for a call that makes the container again when the server is on: the window says it is working. */
  async #change(call: () => Promise<Overview>, refused?: (message: string) => void): Promise<boolean> {
    this.#working = true
    try {
      return await this.#show(call, refused)
    } finally {
      this.#working = false
    }
  }

  /** Read how things are. */
  async load(): Promise<void> {
    await this.#show(() => this.#rpc.overview())
  }

  /**
   * Hear the backend while the window is open: the container changed (it crashed, it was stopped in a terminal), the
   * records did (another window, an agent), or the host connection came back (a change made while it was away
   * reached nobody). Returns a function that stops listening.
   */
  listen(): () => void {
    const again = (): void => {
      void this.load()
      void this.refreshLog()
    }
    const stops = [this.#events.on('changed', again), this.#events.on('host:reconnected', again)]
    return () => stops.forEach((stop) => stop())
  }

  /** Turn the server on or off. */
  setEnabled(enabled: boolean): Promise<boolean> {
    return this.#change(() => this.#rpc.setEnabled({ enabled }))
  }

  /** Make the container again and start it: the way out when the server is not running. */
  apply(): Promise<boolean> {
    return this.#change(() => this.#rpc.apply())
  }

  /** Type something of the next record. */
  editRecord(change: Partial<RecordDraft>): void {
    this.#record = { ...this.#record, ...change }
    this.#recordRefused = null
  }

  /**
   * Add the record that is typed; the fields are empty again once it is. One with a field that is empty or wrong is
   * not sent: its fields say what is wrong with them.
   */
  async addRecord(): Promise<boolean> {
    const { name, address, subdomains } = this.#record
    const problems = this.recordProblems
    if (name.trim() === '' || address.trim() === '' || problems.name !== null || problems.address !== null) return false
    this.#recordRefused = null
    const added = await this.#change(
      () => this.#rpc.addRecord({ name: name.trim(), address: address.trim(), subdomains }),
      (message) => (this.#recordRefused = message)
    )
    if (added) this.#record = NO_RECORD
    return added
  }

  /** Take a record away. */
  async removeRecord(record: DnsRecord): Promise<boolean> {
    this.#removing = new Set([...this.#removing, record.id])
    try {
      return await this.#change(() => this.#rpc.removeRecord({ record: record.id }))
    } finally {
      this.#removing = new Set([...this.#removing].filter((id) => id !== record.id))
    }
  }

  /** Type something of the settings. */
  editSettings(change: Partial<SettingsDraft>): void {
    this.#settings = { ...this.settingsDraft, ...change }
    this.#settingsRefused = null
  }

  /** Forget what is typed of the settings. */
  resetSettings(): void {
    this.#settings = null
    this.#settingsRefused = null
  }

  /** Save the settings that are typed. Ones with a field that is wrong are not sent: the field says what is wrong. */
  async saveSettings(): Promise<boolean> {
    const typed = this.settingsDraft
    const port = portOf(typed.port)
    if (port === undefined || this.settingsProblems.upstreams !== null) return false
    this.#settingsRefused = null
    const saved = await this.#change(
      () => this.#rpc.saveSettings({ port, upstreams: upstreamsOf(typed.upstreams), logQueries: typed.logQueries }),
      (message) => (this.#settingsRefused = message)
    )
    if (saved) this.#settings = null
    return saved
  }

  /** Run the server in the Docker at the address that is typed; empty is this machine's own. */
  async connect(): Promise<void> {
    if (await this.#show(() => this.#rpc.connect({ host: this.hostDraft.trim() }))) this.hostDraft = this.engine.host
  }

  /** Look at the names, the settings or the log. */
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
