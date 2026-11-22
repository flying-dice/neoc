import type { Agent, Api, Chat, Conversation, Host } from 'app:rpc'

/** What the window needs of the backend: the agents, the chats, and a turn in one of them. */
export type AgentsRpc = Pick<Api, 'agents' | 'chats' | 'start' | 'open' | 'send' | 'stop'>
/** Where the backend's events are heard (`host.on`). */
export type AgentsEvents = Pick<Host, 'on'>

/** A piece of the agent's answer, as the backend sends it while the answer comes. */
interface Piece {
  chat: string
  message: number
  text: string
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's Agents: who can be chatted with, the chats, and the one that is open. The chats and what was said in
 * them are the backend's; this holds what the window shows of them, and what is being typed.
 */
export class Agents {
  #agents = $state.raw<Agent[]>([])
  #chats = $state.raw<Chat[]>([])
  // the chat that is open: replaced whole by each answer of the backend, and added to by `piece` events in between
  #open = $state.raw<Conversation | null>(null)
  // why the last call failed: the backend's own message, or the host's
  #failure = $state<string | null>(null)
  /** What is being typed in the open chat. */
  draft = $state('')

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
  /** Whether what is typed can be sent: there is something to say, and the agent is not answering. */
  get sendable(): boolean {
    return this.#open !== null && !this.#open.busy && this.draft.trim() !== ''
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
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /**
   * Hear the backend while the window is open: each piece of an answer, the end of a turn, and the host connection
   * coming back. Returns a function that stops listening.
   */
  listen(): () => void {
    const stops = [
      this.#events.on('piece', (data) => this.#add(data as Piece)),
      // the answer is over (complete, stopped or failed): read the chat as the backend has it
      this.#events.on('turn', (data) => {
        const chat = (data as { chat: string }).chat
        if (this.#open?.chat.id === chat) void this.#show(this.#rpc.open({ chat }))
      }),
      // pieces sent while the connection was away reached nobody
      this.#events.on('host:reconnected', () => void this.load())
    ]
    return () => stops.forEach((stop) => stop())
  }

  #add(piece: Piece): void {
    const open = this.#open
    if (open?.chat.id !== piece.chat) return
    this.#open = {
      ...open,
      messages: open.messages.map((m) => (m.id === piece.message ? { ...m, text: m.text + piece.text } : m))
    }
  }

  startChat(agent: Agent): Promise<boolean> {
    return this.#show(this.#rpc.start({ agent: agent.id }))
  }

  openChat(chat: Chat): Promise<boolean> {
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

  /** Leave the open chat for the list (a narrow window shows one or the other). */
  back(): void {
    this.#open = null
    this.#failure = null
  }
}
