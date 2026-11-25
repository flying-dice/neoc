import type { Api, Host, Part } from 'app:rpc'

/** What the window needs of the backend: only the methods it calls, so a fake has only those to write. */
export type ExampleRpc = Pick<Api, 'parts' | 'greet'>
/** Where the host's events are heard (`host.on`). */
export type ExampleEvents = Pick<Host, 'on'>

/** The backend's own refusal (`ctx:fail`), as against a call that did not get through. */
const isRefusal = (e: unknown): boolean => (e as { code?: unknown } | null)?.code === -32010
/** The sidebar's row that opens the greeting, beside the backend's parts. */
export const HELLO = 'hello'

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's state, in a class of runes: the page reads it and calls its methods, and never the backend itself. The
 * parts are the backend's and are replaced whole by each answer; what is the window's own is here too: which part is
 * looked at, the name typed, the greeting it got, and what went wrong.
 */
export class Example {
  // `raw`: the list is replaced whole by each answer, never edited in place
  #parts = $state.raw<readonly Part[]>([])
  #loaded = $state(false)
  // why the last call did not get through (the host's sentence): the window's, shown in a banner
  #failure = $state<string | null>(null)
  #selected = $state<string | null>(null)
  #name = $state('')
  #greeting = $state<string | null>(null)
  // what the backend refused of the name: said at the field, gone when the name is typed again
  #refusal = $state<string | null>(null)
  #sending = $state(false)

  readonly #rpc: ExampleRpc
  readonly #events: ExampleEvents

  constructor(rpc: ExampleRpc, events: ExampleEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  get parts(): readonly Part[] {
    return this.#parts
  }
  get loaded(): boolean {
    return this.#loaded
  }
  get failure(): string | null {
    return this.#failure
  }
  /** The id of the sidebar row picked: a part's, or `HELLO`; none at first. */
  get selectedId(): string | null {
    return this.#selected
  }
  /** The part looked at; none until one is picked, or when the backend no longer has it. */
  readonly selected: Part | undefined = $derived(this.#parts.find((p) => p.id === this.#selected))
  /** The greeting is open in the detail. */
  readonly saying: boolean = $derived(this.#selected === HELLO)
  get name(): string {
    return this.#name
  }
  get greeting(): string | null {
    return this.#greeting
  }
  get refusal(): string | null {
    return this.#refusal
  }
  get sending(): boolean {
    return this.#sending
  }
  /** There is a name to send, and nothing in flight: the Greet button's `disabled`, the other way round. */
  readonly canGreet: boolean = $derived(this.#name.trim() !== '' && !this.#sending)

  /** Ask the backend for the parts. */
  async load(): Promise<void> {
    try {
      this.#parts = await this.#rpc.parts()
      this.#loaded = true
      this.#failure = null
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Hear the host while the window is open: after the host comes back, ask again. Returns a function that stops. */
  listen(): () => void {
    return this.#events.on('host:reconnected', () => void this.load())
  }

  /** Pick a sidebar row by id (a part's, or `HELLO`), or go back to none. */
  select(id: string | null): void {
    this.#selected = id
  }

  type(name: string): void {
    this.#name = name
    this.#refusal = null
  }

  /** Send the typed name; the answer, or the backend's refusal of it, is kept for the page to show. */
  async greet(): Promise<void> {
    if (!this.canGreet) return
    this.#sending = true
    try {
      this.#greeting = (await this.#rpc.greet({ name: this.#name })).message
      this.#refusal = null
      this.#failure = null
    } catch (e) {
      if (isRefusal(e)) this.#refusal = messageOf(e)
      else this.#failure = messageOf(e)
    } finally {
      this.#sending = false
    }
  }
}
