import type { Api, Host, Outcome, Overview, Restart, Service, Status } from 'app:rpc'
import { matches, search, type Filter } from './format'

/** What the window needs of the backend. */
export type ServicesRpc = Pick<Api, 'overview' | 'start' | 'stop' | 'restart' | 'configure' | 'register' | 'unregister'>
/** Where the backend's events are heard (`host.on`). */
export type ServicesEvents = Pick<Host, 'on'>

export type Control = 'start' | 'stop' | 'restart'

/** A service as it is typed, before it is registered. */
export interface Draft {
  label: string
  program: string
  /** One argument a line. */
  args: string
  description: string
  workingDirectory: string
  autostart: boolean
  restart: Restart | ''
}
const NO_DRAFT: Draft = { label: '', program: '', args: '', description: '', workingDirectory: '', autostart: false, restart: '' }

/** What a call that changed something ended in, for the person: the sentence, and which of the designed states it is. */
export interface Notice {
  status: Exclude<Status, 'done'>
  message: string
  /** The service it was about; empty for a registration. */
  label: string
}

const NOTHING: Overview = {
  manager: { available: false, problem: '', kind: '', level: 'system', installOptions: [], configurable: [], reported: [] },
  services: []
}
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's services. The services are the operating system's: every call answers with the whole overview and
 * this holds the last one; nothing is kept between windows or runs. What is the window's own is here: the filter,
 * the search, which service is looked at, what is typed and not yet registered, what is in flight and what the last
 * change ended in.
 */
export class Services {
  // `raw`: the overview is replaced whole by each answer, never edited in place
  #overview = $state.raw<Overview>(NOTHING)
  #loaded = $state(false)
  // why the last call failed to get through (the host's message), not what the operating system said
  #failure = $state<string | null>(null)
  #notice = $state.raw<Notice | null>(null)
  #filter = $state<Filter>('all')
  #query = $state('')
  #selected = $state<string | null>(null)
  #pending = $state.raw<ReadonlySet<string>>(new Set())
  #registering = $state(false)
  #draft = $state.raw<Draft>(NO_DRAFT)
  #asking = $state<string | null>(null)

  readonly #rpc: ServicesRpc
  readonly #events: ServicesEvents

  constructor(rpc: ServicesRpc, events: ServicesEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  get manager(): Overview['manager'] {
    return this.#overview.manager
  }
  get all(): readonly Service[] {
    return this.#overview.services
  }
  get loaded(): boolean {
    return this.#loaded
  }
  get failure(): string | null {
    return this.#failure
  }
  get notice(): Notice | null {
    return this.#notice
  }
  get filter(): Filter {
    return this.#filter
  }
  get query(): string {
    return this.#query
  }
  /** The services of the filter that match the search. */
  readonly services: readonly Service[] = $derived(search(this.#overview.services.filter((s) => matches(s, this.#filter)), this.#query))
  /** The service that is looked at: gone from the list when the operating system no longer has it. */
  readonly selected: Service | undefined = $derived(this.#overview.services.find((s) => s.label === this.#selected))
  /** The registration form is open. */
  get registering(): boolean {
    return this.#registering
  }
  get draft(): Draft {
    return this.#draft
  }
  /** The service the person is asked about before it is removed. */
  get asking(): Service | undefined {
    return this.#overview.services.find((s) => s.label === this.#asking)
  }
  /** What can be typed in the form: the options the manager keeps. */
  keeps(option: string): boolean {
    return this.manager.installOptions.includes(option)
  }
  /** What can be changed of a service. */
  changes(option: string): boolean {
    return this.manager.configurable.includes(option)
  }
  /** A call is in flight for this service. */
  busy(service: Service): boolean {
    return this.#pending.has(service.label)
  }
  /** A service is being registered. */
  get adding(): boolean {
    return this.#pending.has('')
  }
  /** There is something to register: a name and a program. */
  readonly ready: boolean = $derived(this.#draft.label.trim() !== '' && this.#draft.program.trim() !== '')

  #take(overview: Overview): void {
    this.#overview = overview
    this.#loaded = true
    if (this.#selected !== null && !overview.services.some((s) => s.label === this.#selected)) this.#selected = null
    if (this.#asking !== null && !overview.services.some((s) => s.label === this.#asking)) this.#asking = null
  }

  /** Run a call that answers with an outcome, for `label` (empty for a registration). It never rejects. */
  async #change(label: string, call: () => Promise<Outcome>): Promise<boolean> {
    this.#pending = new Set([...this.#pending, label])
    try {
      const outcome = await call()
      this.#take(outcome.overview)
      this.#failure = null
      this.#notice = outcome.status === 'done' ? null : { status: outcome.status, message: outcome.message, label }
      return outcome.status === 'done'
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    } finally {
      this.#pending = new Set([...this.#pending].filter((l) => l !== label))
    }
  }

  /** Ask the operating system how things are now. */
  async load(): Promise<void> {
    try {
      this.#take(await this.#rpc.overview())
      this.#failure = null
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Hear the host while the window is open. Returns a function that stops listening. */
  listen(): () => void {
    const again = (): void => void this.load()
    const stops = [this.#events.on('changed', again), this.#events.on('host:reconnected', again)]
    return () => stops.forEach((stop) => stop())
  }

  setFilter(filter: Filter): void {
    this.#filter = filter
  }
  setQuery(query: string): void {
    this.#query = query
  }

  /** Look at a service; `null` is back to the list. */
  select(service: Service | null): void {
    this.#selected = service?.label ?? null
    this.#notice = null
  }

  control(service: Service, action: Control): Promise<boolean> {
    this.#notice = null
    return this.#change(service.label, () => this.#rpc[action]({ label: service.label }))
  }

  /** Change settings of a service: only those given. */
  configure(service: Service, change: { autostart?: boolean; description?: string; restart?: Restart }): Promise<boolean> {
    this.#notice = null
    return this.#change(service.label, () => this.#rpc.configure({ label: service.label, ...change }))
  }

  openForm(): void {
    this.#registering = true
    this.#notice = null
  }
  closeForm(): void {
    this.#registering = false
    this.#draft = NO_DRAFT
    this.#notice = null
  }
  edit(change: Partial<Draft>): void {
    this.#draft = { ...this.#draft, ...change }
    if (this.#notice?.label === '') this.#notice = null
  }

  /** Register the service that is typed. The form closes when the operating system has it. */
  async register(): Promise<boolean> {
    if (!this.ready || this.adding) return false
    const sent = this.#draft
    const args = sent.args.split('\n').map((line) => line.trim()).filter((line) => line !== '')
    const done = await this.#change('', () =>
      this.#rpc.register({
        label: sent.label.trim(),
        program: sent.program.trim(),
        ...(args.length > 0 ? { args } : {}),
        ...(sent.description.trim() !== '' ? { description: sent.description.trim() } : {}),
        ...(sent.workingDirectory.trim() !== '' ? { workingDirectory: sent.workingDirectory.trim() } : {}),
        ...(sent.autostart ? { autostart: true } : {}),
        ...(sent.restart !== '' ? { restart: sent.restart } : {})
      })
    )
    if (done) {
      this.#registering = false
      this.#draft = NO_DRAFT
      this.#selected = sent.label.trim()
    }
    return done
  }

  /** Ask before a service is removed: the operating system does not give it back. */
  ask(service: Service): void {
    this.#asking = service.label
  }
  keep(): void {
    this.#asking = null
  }
  /** The answer was Remove. */
  async unregister(): Promise<boolean> {
    const service = this.asking
    if (!service) return false
    this.#asking = null
    const done = await this.#change(service.label, () => this.#rpc.unregister({ label: service.label }))
    if (done) this.#selected = null
    return done
  }
}
