import type { Api, Host, Item, Keychain as Store, Overview } from 'app:rpc'

/** What the window needs of the backend: the overview, the two ways of changing it, and one password at a time. */
export type KeychainRpc = Pick<Api, 'overview' | 'add' | 'reveal' | 'remove'>
/** Where the backend's events are heard (`host.on`). */
export type KeychainEvents = Pick<Host, 'on'>

/** An item as it is typed, before it is added. */
export interface ItemDraft {
  service: string
  account: string
  password: string
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
/** The backend looked at what was asked and said no (`ctx:fail`): the message is about that, not about the call. */
const isRefusal = (e: unknown): boolean => (e as { code?: unknown } | null)?.code === -32010

const NOTHING: Overview = { keychain: { available: false, problem: '' }, items: [] }
const NO_ITEM: ItemDraft = { service: '', account: '', password: '' }

/**
 * One window's keychain. The items are the backend's: every change is a call that answers with the whole overview,
 * and this holds the last one. What is the window's own is here: what is typed and not yet sent, the passwords the
 * person asked to see (each is read when asked for, and forgotten when hidden), and what is in flight.
 */
export class Keychain {
  // `raw`: the overview is replaced whole by each answer, never edited in place
  #overview = $state.raw<Overview>(NOTHING)
  // nothing has been read yet: the window says so instead of "no keychain"
  #loaded = $state(false)
  // why the last call failed (the host's message): not what was typed, which is said at its form
  #failure = $state<string | null>(null)
  // what the backend refused of the item as it was sent: it goes when the item is typed again
  #refused = $state<string | null>(null)
  #draft = $state.raw<ItemDraft>(NO_ITEM)
  #adding = $state(false)
  // the passwords that are shown, by the id of their item
  #shown = $state.raw<ReadonlyMap<number, string>>(new Map())
  // why an item's password could not be shown, by the id of the item
  #problems = $state.raw<ReadonlyMap<number, string>>(new Map())
  // the ids of the items a call is in flight for
  #busy = $state.raw<ReadonlySet<number>>(new Set())
  // the item the person is asked about before it is deleted
  #asking = $state.raw<Item | null>(null)

  readonly #rpc: KeychainRpc
  readonly #events: KeychainEvents

  constructor(rpc: KeychainRpc, events: KeychainEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  /** Whether this machine has a keychain the app can use, and why not. */
  get store(): Store {
    return this.#overview.keychain
  }
  get items(): readonly Item[] {
    return this.#overview.items
  }
  /** The first answer has arrived. */
  get loaded(): boolean {
    return this.#loaded
  }
  get failure(): string | null {
    return this.#failure
  }
  /** The item that is being typed. */
  get draft(): ItemDraft {
    return this.#draft
  }
  /** The backend's word on the item that was sent and not added (the keychain has it already). */
  get refused(): string | null {
    return this.#refused
  }
  /** The item that is typed is being added. */
  get adding(): boolean {
    return this.#adding
  }
  /** The item the person is asked about before it is deleted. */
  get asking(): Item | null {
    return this.#asking
  }

  /** There is something to add: what the password is for, and whose it is. The password itself may be left out. */
  readonly ready: boolean = $derived(this.#draft.service.trim() !== '' && this.#draft.account.trim() !== '')

  /** The password of an item, while it is shown. */
  password(item: Item): string | null {
    return this.#shown.get(item.id) ?? null
  }
  /** Why the password of an item could not be shown. */
  problem(item: Item): string | null {
    return this.#problems.get(item.id) ?? null
  }
  /** A call is in flight for this item. */
  busy(item: Item): boolean {
    return this.#busy.has(item.id)
  }

  /** Take an answer of the backend as how things are now: what was shown of an item that is gone is forgotten. */
  #take(overview: Overview): void {
    this.#overview = overview
    this.#loaded = true
    const there = new Set(overview.items.map((item) => item.id))
    this.#shown = new Map([...this.#shown].filter(([id]) => there.has(id)))
    this.#problems = new Map([...this.#problems].filter(([id]) => there.has(id)))
    if (this.#asking && !there.has(this.#asking.id)) this.#asking = null
  }

  /**
   * Run a call that answers with the overview. It never rejects: a failure is kept, to be shown. What the backend
   * refused goes to `refused`, to be said where it was asked; everything else is the window's.
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

  /** Run a call for one item: the item is busy until it is answered. */
  async #with<T>(item: Item, call: () => Promise<T>): Promise<T> {
    this.#busy = new Set([...this.#busy, item.id])
    try {
      return await call()
    } finally {
      this.#busy = new Set([...this.#busy].filter((id) => id !== item.id))
    }
  }

  #say(item: Item, problem: string | null): void {
    const problems = new Map(this.#problems)
    if (problem === null) problems.delete(item.id)
    else problems.set(item.id, problem)
    this.#problems = problems
  }

  /** Read how things are. */
  async load(): Promise<void> {
    await this.#show(() => this.#rpc.overview())
  }

  /**
   * Hear the backend while the window is open: an agent added an item, or the host connection came back (a change
   * made while it was away reached nobody). Returns a function that stops listening.
   */
  listen(): () => void {
    const again = (): void => void this.load()
    const stops = [this.#events.on('changed', again), this.#events.on('host:reconnected', again)]
    return () => stops.forEach((stop) => stop())
  }

  /** Type something of the next item. */
  edit(change: Partial<ItemDraft>): void {
    this.#draft = { ...this.#draft, ...change }
    this.#refused = null
  }

  /**
   * Add the item that is typed; the fields are empty again once it is. With a password it is kept in the keychain;
   * without one the item is one the keychain has already. One with no service or no account is not sent.
   */
  async add(): Promise<boolean> {
    if (!this.ready || this.#adding) return false
    const { service, account, password } = this.#draft
    this.#refused = null
    this.#adding = true
    try {
      const added = await this.#show(
        () => this.#rpc.add({ service: service.trim(), account: account.trim(), ...(password === '' ? {} : { password }) }),
        (message) => (this.#refused = message)
      )
      if (added) this.#draft = NO_ITEM
      return added
    } finally {
      this.#adding = false
    }
  }

  /** Read the password of an item from the keychain and show it. The machine may ask its person first. */
  async reveal(item: Item): Promise<boolean> {
    this.#say(item, null)
    return this.#with(item, async () => {
      try {
        const { password } = await this.#rpc.reveal({ item: item.id })
        this.#shown = new Map([...this.#shown, [item.id, password]])
        this.#failure = null
        return true
      } catch (e) {
        // the keychain's word on this item is said at the item; a call that did not get through is the window's
        if (isRefusal(e)) this.#say(item, messageOf(e))
        else this.#failure = messageOf(e)
        return false
      }
    })
  }

  /** Stop showing the password of an item, and forget it. */
  hide(item: Item): void {
    this.#shown = new Map([...this.#shown].filter(([id]) => id !== item.id))
  }

  /** Ask before an item is deleted: the keychain does not give it back. */
  ask(item: Item): void {
    this.#asking = item
  }

  /** The answer was Cancel. */
  keep(): void {
    this.#asking = null
  }

  /** The answer was Delete: the item that was asked about goes from the keychain and from the list. */
  async remove(): Promise<boolean> {
    const item = this.#asking
    if (item === null) return false
    this.#asking = null
    this.#say(item, null)
    return this.#with(item, () =>
      this.#show(
        () => this.#rpc.remove({ item: item.id }),
        (message) => this.#say(item, message)
      )
    )
  }
}
