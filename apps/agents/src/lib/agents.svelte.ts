import type { Agent, Api, Chat, Conversation, Host, Item } from 'app:rpc'

/** What the window needs of the backend: the agents, the chats, and a turn in one of them. */
export type AgentsRpc = Pick<
  Api,
  'agents' | 'addAgent' | 'updateAgent' | 'removeAgent' | 'chats' | 'start' | 'open' | 'send' | 'answer' | 'act' | 'stop' | 'removeChat'
>
/** Where the backend's events are heard (`host.on`). */
export type AgentsEvents = Pick<Host, 'on'>

/** An item of the open chat, new or changed, as the backend sends it during a turn. */
interface Changed {
  chat: string
  item: Item
}
/** Text added to an item, as the backend sends it while the text comes. */
interface Piece {
  chat: string
  item: number
  text: string
}
/** The state the agent shares, as JSON text, when it changed. */
interface Shared {
  chat: string
  state: string
}

/** The form an agent is added or changed in. `agent` is the one being changed; without it the form adds one. */
export interface AgentForm {
  agent: Agent | null
  name: string
  url: string
  /** A new token. Empty leaves the token the agent has, when it has one. */
  token: string
  /** Remove the token the agent has. */
  forget: boolean
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's Agents: who can be chatted with, the chats, and the one that is open. The agents, the chats and what
 * was said in them are the backend's; this holds what the window shows of them, and what is being typed.
 */
export class Agents {
  #agents = $state.raw<Agent[]>([])
  #chats = $state.raw<Chat[]>([])
  // the chat that is open: replaced whole by each answer of the backend, and changed by its events in between
  #open = $state.raw<Conversation | null>(null)
  // why the last call failed: the backend's own message, or the host's
  #failure = $state<string | null>(null)
  #loaded = $state(false)
  /** What is being typed in the open chat. */
  draft = $state('')
  /** The agent form, while it is open. */
  form = $state<AgentForm | null>(null)
  /** What the person asked to delete, until they say yes or no: the open chat, or the agent in the form. */
  asking = $state<'chat' | 'agent' | null>(null)

  readonly #rpc: AgentsRpc
  readonly #events: AgentsEvents

  constructor(rpc: AgentsRpc, events: AgentsEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  get agents(): readonly Agent[] {
    return this.#agents
  }
  get chats(): readonly Chat[] {
    return this.#chats
  }
  get open(): Conversation | null {
    return this.#open
  }
  get failure(): string | null {
    return this.#failure
  }
  /** The agents and chats have been read once: an empty list is empty, not still coming. */
  get loaded(): boolean {
    return this.#loaded
  }
  /** Whether what is typed can be sent: there is something to say, and the agent is neither answering nor asking. */
  get sendable(): boolean {
    return this.#open !== null && !this.#open.busy && !this.#open.waiting && this.draft.trim() !== ''
  }

  /** Run a call that answers with a chat. It never rejects: a failure is kept, to be shown. */
  async #show(call: Promise<Conversation>): Promise<boolean> {
    try {
      this.#open = await call
      this.#chats = await this.#rpc.chats()
      this.#failure = null
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** Read the agents and the chats, and the open chat again (it may have moved on while nobody was listening). */
  async load(): Promise<void> {
    try {
      ;[this.#agents, this.#chats] = await Promise.all([this.#rpc.agents(), this.#rpc.chats()])
      const last = this.#open?.chat.id
      if (last !== undefined && this.#chats.some((c) => c.id === last)) this.#open = await this.#rpc.open({ chat: last })
      this.#failure = null
      this.#loaded = true
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /**
   * Hear the backend while the window is open: what the agent says and does, the end of a turn, and the host
   * connection coming back. Returns a function that stops listening.
   */
  listen(): () => void {
    const stops = [
      this.#events.on('item', (data) => this.#change(data as Changed)),
      this.#events.on('piece', (data) => this.#add(data as Piece)),
      this.#events.on('state', (data) => {
        const shared = data as Shared
        if (this.#open?.chat.id === shared.chat) this.#open = { ...this.#open, state: shared.state }
      }),
      // the turn is over (complete, stopped or failed): read the chat as the backend has it
      this.#events.on('turn', (data) => {
        const chat = (data as { chat: string }).chat
        if (this.#open?.chat.id === chat) void this.#show(this.#rpc.open({ chat }))
      }),
      // events sent while the connection was away reached nobody
      this.#events.on('host:reconnected', () => void this.load())
    ]
    return () => stops.forEach((stop) => stop())
  }

  #change({ chat, item }: Changed): void {
    const open = this.#open
    if (open?.chat.id !== chat) return
    const known = open.items.some((i) => i.id === item.id)
    this.#open = { ...open, items: known ? open.items.map((i) => (i.id === item.id ? item : i)) : [...open.items, item] }
  }

  #add(piece: Piece): void {
    const open = this.#open
    if (open?.chat.id !== piece.chat) return
    this.#open = { ...open, items: open.items.map((i) => (i.id === piece.item ? { ...i, text: i.text + piece.text } : i)) }
  }

  startChat(agent: Agent): Promise<boolean> {
    this.form = null
    return this.#show(this.#rpc.start({ agent: agent.id }))
  }

  openChat(chat: Chat): Promise<boolean> {
    this.form = null
    return this.#show(this.#rpc.open({ chat: chat.id }))
  }

  /** Send what is typed. What was typed comes back when it could not be sent. */
  async send(): Promise<void> {
    if (!this.sendable || this.#open === null) return
    const text = this.draft
    this.draft = ''
    if (!(await this.#show(this.#rpc.send({ chat: this.#open.chat.id, text })))) this.draft = text
  }

  /** Stop the agent's answer in the open chat; what it said so far stays. */
  async stop(): Promise<void> {
    if (this.#open !== null) await this.#show(this.#rpc.stop({ chat: this.#open.chat.id }))
  }

  /** Answer a question the agent stopped at, with what the person wrote, or decline it. */
  async answer(item: Item, answer: string | null): Promise<void> {
    if (this.#open === null) return
    const chat = this.#open.chat.id
    const params = answer === null ? { chat, item: item.id, decline: true } : { chat, item: item.id, payload: JSON.stringify(answer) }
    await this.#show(this.#rpc.answer(params))
  }

  /** Tell the agent what the person did on a surface it drew. */
  async act(item: Item, action: unknown): Promise<void> {
    if (this.#open === null || this.#open.busy) return
    await this.#show(this.#rpc.act({ chat: this.#open.chat.id, item: item.id, action: JSON.stringify(action) }))
  }

  /** Delete the open chat. */
  async removeChat(): Promise<void> {
    this.asking = null
    if (this.#open === null) return
    try {
      this.#chats = await this.#rpc.removeChat({ chat: this.#open.chat.id })
      this.#open = null
      this.#failure = null
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Open the form: to add an agent, or to change the one given. */
  edit(agent: Agent | null): void {
    this.#failure = null
    this.form = { agent, name: agent?.name ?? '', url: agent?.url ?? '', token: '', forget: false }
  }

  /** Whether the form can be saved: it has a name and an endpoint. What is wrong with them is the backend's to say. */
  get savable(): boolean {
    return this.form !== null && this.form.name.trim() !== '' && this.form.url.trim() !== ''
  }

  /** Save the form. It stays open, with the reason, when the backend refuses it. */
  async save(): Promise<void> {
    const form = this.form
    if (form === null || !this.savable) return
    const token = form.token === '' ? {} : { token: form.token }
    try {
      this.#agents =
        form.agent === null
          ? await this.#rpc.addAgent({ name: form.name, url: form.url, ...token })
          : await this.#rpc.updateAgent({ agent: form.agent.id, name: form.name, url: form.url, forget: form.forget, ...token })
      this.#chats = await this.#rpc.chats()
      this.#failure = null
      this.form = null
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Remove the agent the form is changing, and its token. Its chats stay. */
  async remove(): Promise<void> {
    this.asking = null
    const agent = this.form?.agent
    if (!agent) return
    try {
      this.#agents = await this.#rpc.removeAgent({ agent: agent.id })
      this.#failure = null
      this.form = null
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Leave the open chat or the form for the list (a narrow window shows one or the other). */
  back(): void {
    this.#open = null
    this.form = null
    this.#failure = null
  }
}
