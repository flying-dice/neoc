type Container = VosApp.Containers.Container
type Details = VosApp.Containers.Details
type Engine = VosApp.Containers.Engine
type Logs = VosApp.Containers.Logs
type Made = VosApp.Containers.Made
type Network = VosApp.Containers.Network
type Overview = VosApp.Containers.Overview
type Pulling = VosApp.Containers.PullingParams
type Stack = VosApp.Containers.Stack
type Volume = VosApp.Containers.Volume
import { blankDraft, copyName, draftOf, specOf, type Draft } from './spec'

/**
 * What the window needs of the backend: the overview, each way of changing it, a container's details and logs, and the
 * networks and volumes a new container can use (the app's private methods on the bus, `_overview` ...: `App.svelte`
 * sends them).
 */
export interface ContainersRpc {
  overview(): Promise<Overview>
  connect(params: VosApp.Containers.ConnectParams): Promise<Overview>
  start(params: VosApp.Containers.StartParams): Promise<Overview>
  stop(params: VosApp.Containers.StopParams): Promise<Overview>
  restart(params: VosApp.Containers.RestartParams): Promise<Overview>
  remove(params: VosApp.Containers.RemoveParams): Promise<Overview>
  create(params: VosApp.Containers.CreateParams): Promise<Made>
  recreate(params: VosApp.Containers.RecreateParams): Promise<Made>
  networks(): Promise<Network[]>
  volumes(): Promise<Volume[]>
  startStack(params: VosApp.Containers.StackStartParams): Promise<Overview>
  stopStack(params: VosApp.Containers.StackStopParams): Promise<Overview>
  restartStack(params: VosApp.Containers.StackRestartParams): Promise<Overview>
  removeStack(params: VosApp.Containers.StackRemoveParams): Promise<Overview>
  details(params: VosApp.Containers.DetailsParams): Promise<Details>
  logs(params: VosApp.Containers.LogsParams): Promise<Logs>
}
/**
 * Where the window hears that Docker changed (the backend's `changed`, and the host's `host:reconnected`), and how the
 * pull of an image goes while a container waits for it (the backend's `_pulling`).
 */
export interface ContainersEvents {
  on(name: 'changed' | 'host:reconnected', fn: () => void): () => void
  on(name: 'pulling', fn: (progress: Pulling) => void): () => void
}

/** What can be done to a container, and to a stack. Removing a container asks first, with its own choices (`ask`). */
export type Action = 'start' | 'stop' | 'restart' | 'remove'
/** What the form does: make a new container, change one (it is made again), or make a copy of one. */
export type Mode = 'new' | 'edit' | 'duplicate'

/** The form a container is made or changed in, while it is open. */
export interface ContainerForm {
  mode: Mode
  /** The container it changes or copies; none for a new one. */
  of: Container | null
  draft: Draft
  /** Why the backend refused it, in its words. */
  failure: string | null
  /** It is being saved: the image pulled, the container made. */
  saving: boolean
  /** How the pull of its image goes, while it is saved. */
  pulling: Pulling | null
}

/** The question before a container is removed, and what is chosen with it. */
export interface Removal {
  container: Container
  /** Remove it while it runs: Docker kills it first. */
  force: boolean
  /** Its anonymous volumes go with it. */
  volumes: boolean
  /** Why Docker refused, in its words. */
  failure: string | null
  busy: boolean
}
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
  /** The form a container is made or changed in, while it is open. */
  form = $state<ContainerForm | null>(null)
  /** The question before a container is removed, while it is asked. */
  removal = $state<Removal | null>(null)
  #networks = $state.raw<readonly Network[]>([])
  #volumes = $state.raw<readonly Volume[]>([])

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
  /** Docker's networks, for the form; read when it opens. */
  get networks(): readonly Network[] {
    return this.#networks
  }
  /** Docker's volumes, for the form; read when it opens. */
  get volumes(): readonly Volume[] {
    return this.#volumes
  }
  /** The form can be saved: it names an image, and is not being saved. What else is wrong is the backend's to say. */
  get savable(): boolean {
    return this.form !== null && !this.form.saving && this.form.draft.image.trim() !== ''
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
    // the form stays: a container it changes goes away while it is saved, and one that went is the backend's to say
    if (this.#inspected !== null && !overview.containers.some((c) => c.id === this.#inspected)) this.#leave()
    const asked = this.removal
    if (asked && !asked.busy && !overview.containers.some((c) => c.id === asked.container.id)) this.removal = null
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
    // the pull is the form's while it saves a container of that image; any other window's pull is not
    const pulling = (progress: Pulling): void => {
      const form = this.form
      if (form?.saving && progress.image === form.draft.image.trim()) form.pulling = progress
    }
    const stops = [this.#events.on('changed', again), this.#events.on('host:reconnected', again), this.#events.on('pulling', pulling)]
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

  /** Start, stop or restart a container. */
  act(container: Container, action: Exclude<Action, 'remove'>): Promise<boolean> {
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

  /** Stop looking at the container that is looked at. */
  #leave(): void {
    this.#inspected = null
    this.#logs = null
    this.#details = null
  }

  /** Go back to the list. */
  close(): void {
    this.#leave()
    this.form = null
    this.removal = null
  }

  /** Read the networks and volumes the form offers. A failure is kept, to be shown; the form opens anyway. */
  async #choices(): Promise<void> {
    try {
      const [networks, volumes] = await Promise.all([this.#rpc.networks(), this.#rpc.volumes()])
      this.#networks = networks
      this.#volumes = volumes
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Open the form for a new container. */
  async compose(): Promise<void> {
    this.removal = null
    this.form = { mode: 'new', of: null, draft: blankDraft(), failure: null, saving: false, pulling: null }
    this.#browsing = false
    await this.#choices()
  }

  /** Open the form filled from a container: to change it (it is made again) or to make a copy of it, by another name. */
  async open(container: Container, mode: 'edit' | 'duplicate'): Promise<void> {
    this.removal = null
    try {
      const [details] = await Promise.all([this.#rpc.details({ container: container.id }), this.#choices()])
      const draft = draftOf(details.spec, this.#volumes.map((v) => v.name))
      if (mode === 'duplicate') {
        draft.name = copyName(container.name, this.all.map((c) => c.name))
        draft.start = true
      }
      this.form = { mode, of: container, draft, failure: null, saving: false, pulling: null }
      this.#failure = null
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Leave the form, for the container it was opened from or else the list. */
  cancel(): void {
    if (this.form?.saving) return
    this.form = null
  }

  /**
   * Save the form: the container is made (or made again), and looked at. The form stays open, with the backend's
   * reason, when it is refused.
   */
  async save(): Promise<void> {
    const form = this.form
    if (form === null || !this.savable) return
    form.saving = true
    form.failure = null
    form.pulling = null
    const spec = specOf(form.draft)
    try {
      const made =
        form.mode === 'edit' && form.of !== null
          ? await this.#rpc.recreate({ container: form.of.id, spec })
          : await this.#rpc.create({ spec })
      this.form = null
      this.#take(made.overview)
      this.#failure = null
      const container = made.overview.containers.find((c) => c.id === made.container)
      if (container) await this.inspect(container)
    } catch (e) {
      form.failure = messageOf(e)
    } finally {
      form.saving = false
      form.pulling = null
    }
  }

  /** Ask before a container is removed: nothing is chosen yet. */
  ask(container: Container): void {
    this.form = null
    this.removal = { container, force: false, volumes: false, failure: null, busy: false }
  }

  /** Remove the container that was asked about, as chosen. The question stays, with Docker's reason, when it refuses. */
  async remove(): Promise<void> {
    const removal = this.removal
    if (removal === null || removal.busy) return
    removal.busy = true
    removal.failure = null
    const { container, force, volumes } = removal
    try {
      this.#take(await this.#rpc.remove({ container: container.id, force, volumes }))
      this.#failure = null
      this.removal = null
    } catch (e) {
      removal.failure = messageOf(e)
    } finally {
      removal.busy = false
    }
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
