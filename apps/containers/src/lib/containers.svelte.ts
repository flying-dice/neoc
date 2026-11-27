type Container = VosApp.Containers.Container
type Details = VosApp.Containers.Details
type Engine = VosApp.Containers.Engine
type Logs = VosApp.Containers.Logs
type Overview = VosApp.Containers.Overview
type Stack = VosApp.Containers.Stack

/**
 * What the window needs of the backend: the overview, each way of changing it, and a container's details and logs (the
 * app's private methods on the bus, `_overview` ...: `App.svelte` sends them).
 */
export interface ContainersRpc {
  overview(): Promise<Overview>
  connect(params: VosApp.Containers.ConnectParams): Promise<Overview>
  start(params: VosApp.Containers.StartParams): Promise<Overview>
  stop(params: VosApp.Containers.StopParams): Promise<Overview>
  restart(params: VosApp.Containers.RestartParams): Promise<Overview>
  remove(params: VosApp.Containers.RemoveParams): Promise<Overview>
  startStack(params: VosApp.Containers.StackStartParams): Promise<Overview>
  stopStack(params: VosApp.Containers.StackStopParams): Promise<Overview>
  restartStack(params: VosApp.Containers.StackRestartParams): Promise<Overview>
  removeStack(params: VosApp.Containers.StackRemoveParams): Promise<Overview>
  details(params: VosApp.Containers.DetailsParams): Promise<Details>
  logs(params: VosApp.Containers.LogsParams): Promise<Logs>
}
/** Where the window hears that Docker changed: the backend's `changed`, and the host's `host:reconnected`. */
export interface ContainersEvents {
  on(name: 'changed' | 'host:reconnected', fn: () => void): () => void
}

/** What can be done to a container, and to a stack. */
export type Action = 'start' | 'stop' | 'restart' | 'remove'
/** Which containers the list shows: all of them, or those of one stack. */
export type Scope = { kind: 'all' } | { kind: 'stack'; name: string }
/** What of a container is looked at. */
export type Tab = 'logs' | 'details'

export const ALL: Scope = { kind: 'all' }
export const stackScope = (name: string): Scope => ({ kind: 'stack', name })
export const sameScope = (a: Scope, b: Scope): boolean =>
  a.kind === 'all' ? b.kind === 'all' : b.kind === 'stack' && a.name === b.name

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

const NOTHING: Overview = {
  engine: { reachable: false, version: '', host: '', problem: '' },
  containers: [],
  stacks: []
}

/**
 * One window's Containers. The containers and the stacks are Docker's: every change is a call that answers with the
 * whole overview, and this holds the last one. What is the window's own is here: which stack is shown, which
 * container is looked at, and what is in flight.
 */
export class Containers {
  // `raw`: the overview is replaced whole by each answer, never edited in place
  #overview = $state.raw<Overview>(NOTHING)
  // nothing has been read yet: the window says so instead of "Docker is not running"
  #loaded = $state(false)
  // why the last call failed: the backend's own message, or the host's. It stays until a call works
  #failure = $state<string | null>(null)
  #scope = $state.raw<Scope>(ALL)
  // a window too narrow for both shows the stacks until one is picked
  #browsing = $state(true)
  // the ids of the containers, and the names of the stacks (as `stack:<name>`), a call is in flight for
  #pending = $state.raw<ReadonlySet<string>>(new Set())
  // the container that is looked at, by id
  #inspected = $state<string | null>(null)
  #tab = $state<Tab>('logs')
  #logs = $state.raw<Logs | null>(null)
  #details = $state.raw<Details | null>(null)
  /** What is typed as the address of another Docker. */
  hostDraft = $state('')

  readonly #rpc: ContainersRpc
  readonly #events: ContainersEvents

  constructor(rpc: ContainersRpc, events: ContainersEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  get engine(): Engine {
    return this.#overview.engine
  }
  /** The first answer has arrived. */
  get loaded(): boolean {
    return this.#loaded
  }
  get stacks(): readonly Stack[] {
    return this.#overview.stacks
  }
  /** Every container, whatever is shown. */
  get all(): readonly Container[] {
    return this.#overview.containers
  }
  get scope(): Scope {
    return this.#scope
  }
  get failure(): string | null {
    return this.#failure
  }
  /** The stacks are in front, not the containers (it matters only in a window too narrow for both). */
  get browsing(): boolean {
    return this.#browsing
  }
  get tab(): Tab {
    return this.#tab
  }
  /** The logs of the container that is looked at, once they are read. */
  get logs(): Logs | null {
    return this.#logs
  }
  /** The details of the container that is looked at, once they are read. */
  get details(): Details | null {
    return this.#details
  }

  /** The stack that is shown, when one is. */
  readonly stack: Stack | undefined = $derived.by(() => {
    const scope = this.#scope
    return scope.kind === 'stack' ? this.#overview.stacks.find((s) => s.name === scope.name) : undefined
  })
  /** The containers of what is shown. */
  readonly containers: readonly Container[] = $derived.by(() => {
    const scope = this.#scope
    return scope.kind === 'all' ? this.#overview.containers : this.#overview.containers.filter((c) => c.stack === scope.name)
  })
  /** The name of what is shown. */
  readonly title: string = $derived(this.#scope.kind === 'all' ? 'All Containers' : this.#scope.name)
  /** The container that is looked at. */
  readonly inspected: Container | undefined = $derived(this.#overview.containers.find((c) => c.id === this.#inspected))
  /** New output may arrive for what is shown: the logs of a container that is going. */
  readonly following: boolean = $derived(this.inspected !== undefined && this.#tab === 'logs' && this.inspected.state === 'running')

  /** A call is in flight for this container. */
  busy(container: Container): boolean {
    return this.#pending.has(container.id) || (container.stack !== '' && this.#pending.has(`stack:${container.stack}`))
  }
  /** A call is in flight for this stack. */
  stackBusy(stack: Stack): boolean {
    return this.#pending.has(`stack:${stack.name}`)
  }

  /** Take an answer of the backend as what Docker has now. */
  #take(overview: Overview): void {
    this.#overview = overview
    // the address field starts as what is chosen
    if (!this.#loaded) this.hostDraft = overview.engine.host
    this.#loaded = true
    // what was shown or looked at is gone (removed here, in another window, or in a terminal)
    const scope = this.#scope
    if (scope.kind === 'stack' && !overview.stacks.some((s) => s.name === scope.name)) {
      this.#scope = ALL
      this.#browsing = true
    }
    if (this.#inspected !== null && !overview.containers.some((c) => c.id === this.#inspected)) this.close()
  }

  /** Run a call that answers with the overview, for `key`. It never rejects: a failure is kept, to be shown. */
  async #show(call: () => Promise<Overview>, key?: string): Promise<boolean> {
    if (key !== undefined) this.#pending = new Set([...this.#pending, key])
    try {
      this.#take(await call())
      this.#failure = null
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    } finally {
      if (key !== undefined) this.#pending = new Set([...this.#pending].filter((k) => k !== key))
    }
  }

  /** Read what Docker has. */
  async load(): Promise<void> {
    await this.#show(() => this.#rpc.overview())
  }

  /**
   * Hear the backend while the window is open: Docker changed (here, in another window, in a terminal), or the host
   * connection came back (a change made while it was away reached nobody). Returns a function that stops listening.
   */
  listen(): () => void {
    const again = (): void => {
      void this.load()
      void this.refresh()
    }
    const stops = [this.#events.on('changed', again), this.#events.on('host:reconnected', again)]
    return () => stops.forEach((stop) => stop())
  }

  /** Show every container, or those of one stack. */
  select(scope: Scope): void {
    this.#scope = scope
    this.#browsing = false
    this.close()
  }

  /** Leave the containers for the stacks (a narrow window shows one or the other). */
  back(): void {
    this.#browsing = true
    this.close()
  }

  /** Do something to a container. */
  act(container: Container, action: Action): Promise<boolean> {
    return this.#show(() => this.#rpc[action]({ container: container.id }), container.id)
  }

  /** Do the same to every container of a stack. */
  actOnStack(stack: Stack, action: Action): Promise<boolean> {
    const method = ({ start: 'startStack', stop: 'stopStack', restart: 'restartStack', remove: 'removeStack' } as const)[action]
    return this.#show(() => this.#rpc[method]({ stack: stack.name }), `stack:${stack.name}`)
  }

  /** Manage the Docker at the address that is typed; empty is this machine's own. */
  async connect(): Promise<void> {
    if (await this.#show(() => this.#rpc.connect({ host: this.hostDraft.trim() }))) this.hostDraft = this.engine.host
  }

  /** Look at a container: its logs first. */
  async inspect(container: Container): Promise<void> {
    this.#inspected = container.id
    this.#tab = 'logs'
    this.#logs = null
    this.#details = null
    await this.refresh()
  }

  /** Look at the logs of the container, or at what it was made with. */
  async show(tab: Tab): Promise<void> {
    this.#tab = tab
    await this.refresh()
  }

  /** Go back to the list. */
  close(): void {
    this.#inspected = null
    this.#logs = null
    this.#details = null
  }

  /** Read again what is shown of the container that is looked at. An answer for a container left meanwhile is dropped. */
  async refresh(): Promise<void> {
    const container = this.#inspected
    if (container === null) return
    try {
      if (this.#tab === 'logs') {
        const logs = await this.#rpc.logs({ container })
        if (this.#inspected === container) this.#logs = logs
      } else {
        const details = await this.#rpc.details({ container })
        if (this.#inspected === container) this.#details = details
      }
      if (this.#inspected === container) this.#failure = null
    } catch (e) {
      if (this.#inspected === container) this.#failure = messageOf(e)
    }
  }
}
