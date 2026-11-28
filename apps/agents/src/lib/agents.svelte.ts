import { addParamsOf, agentDraftOf, savable, updateParamsOf, type AgentDraft } from './form'
import { needsDocker, type Kind } from './kinds'

type Agent = VosApp.Agents.Agent
type Chat = VosApp.Agents.Chat
type Conversation = VosApp.Agents.Conversation
type Engine = VosApp.Agents.Engine
type Item = VosApp.Agents.Item
type Secret = VosApp.Agents.Secret
type SkillSummary = VosApp.Agents.SkillSummary

/**
 * What the window needs of the backend: the agents, the chats, and a turn in one of them, and what a form offers to
 * choose from (Docker, its images, the Skills app's skills, the keychain's items): the app's private methods on the
 * bus, `_agents` ...: `App.svelte` sends them.
 */
export interface AgentsRpc {
  agents(): Promise<Agent[]>
  addAgent(params: VosApp.Agents.AddAgentParams): Promise<Agent[]>
  updateAgent(params: VosApp.Agents.UpdateAgentParams): Promise<Agent[]>
  removeAgent(params: VosApp.Agents.RemoveAgentParams): Promise<Agent[]>
  engine(): Promise<Engine>
  connect(params: VosApp.Agents.ConnectParams): Promise<Engine>
  secrets(): Promise<Secret[]>
  skills(): Promise<VosApp.Agents.SkillChoice>
  images(): Promise<VosApp.Agents.ImageChoice>
  chats(): Promise<Chat[]>
  open(params: VosApp.Agents.OpenParams): Promise<Conversation>
  readItem(params: VosApp.Agents.ReadItemParams): Promise<Item>
  send(params: VosApp.Agents.SendParams): Promise<Conversation>
  answer(params: VosApp.Agents.AnswerParams): Promise<Conversation>
  stop(params: VosApp.Agents.StopParams): Promise<Conversation>
  removeChat(params: VosApp.Agents.RemoveChatParams): Promise<Chat[]>
}
/**
 * Where the window hears the backend's notifications (`item`, `piece`, `item_changed`, `turn`: `App.svelte` wires them
 * to the bus) and the host's events (`host:reconnected`).
 */
export interface AgentsEvents {
  on(name: string, fn: (data: unknown) => void): () => void
}

/** An item of the open chat, new or changed, as the backend sends it during a turn. */
interface Changed {
  chat: string
  item: Item
}
/** An item that changed and is too large to send: the window reads it. */
interface Reference {
  chat: string
  item: number
}
/** Text added to an item, as the backend sends it while the text comes. */
interface Piece {
  chat: string
  item: number
  text: string
}

/** The form an agent is added or changed in (lib/form.ts). */
export type AgentForm = AgentDraft

/** What the detail column shows besides a chat: the type picker, or the agent form. */
export type Pane = 'types' | 'agent' | null

/** What a form offers to choose from, and why it offers nothing when it could not read it. */
export interface Choices<T> {
  items: readonly T[]
  problem: string | null
}

const readKey = (chat: string, item: number): string => `${chat}/${item}`
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
const none = <T>(): Choices<T> => ({ items: [], problem: null })

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
  #engine = $state.raw<Engine | null>(null)
  #secrets = $state.raw<Choices<Secret>>(none())
  #skills = $state.raw<Choices<SkillSummary>>(none())
  #images = $state.raw<Choices<string>>(none())
  /** What is being typed in the open chat. */
  draft = $state('')
  /** The agent form, while it is open. */
  form = $state<AgentForm | null>(null)
  /** The type picker is open: the first step of adding an agent. */
  picking = $state(false)
  /** The address typed for another Docker. */
  hostDraft = $state('')
  /** What the person asked to delete, until they say yes or no: the open chat, or the agent in the form. */
  asking = $state<'chat' | 'agent' | null>(null)

  readonly #rpc: AgentsRpc
  readonly #events: AgentsEvents
  // the items being read, by chat and id: `again` says the item changed since the read began, so it is read once more
  readonly #reading = new Map<string, { again: boolean }>()
  // how many whole conversations have been installed (an answer of the backend, not a change of the one shown)
  #installed = 0

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
  /** Whether Docker can be reached, as the backend last said. */
  get engine(): Engine | null {
    return this.#engine
  }
  /** The keychain's items a variable or a header can take its value from. */
  get secrets(): Choices<Secret> {
    return this.#secrets
  }
  /** The Skills app's skills a container agent can use. */
  get skills(): Choices<SkillSummary> {
    return this.#skills
  }
  /** The images Docker has, for a container agent's image. */
  get images(): Choices<string> {
    return this.#images
  }
  /** What the detail column shows besides a chat; null for a chat, or nothing. */
  get pane(): Pane {
    return this.form ? 'agent' : this.picking ? 'types' : null
  }
  /** Whether what is typed can be sent: there is something to say, and the agent is not answering. */
  get sendable(): boolean {
    return this.#open !== null && !this.#open.busy && this.draft.trim() !== ''
  }
  /** Whether the form can be saved: a name, and what its type cannot do without. What is wrong is the backend's to say. */
  get savable(): boolean {
    return this.form !== null && savable(this.form)
  }

  /** The agent with that id, while it is there. */
  agentOf(id: string): Agent | null {
    return this.#agents.find((a) => a.id === id) ?? null
  }

  /**
   * Docker cannot be reached, and what the detail column shows needs it: a container agent's form, or a chat with one.
   * Agents of the other types never need it.
   */
  get dockerDown(): boolean {
    const kind = this.form?.kind ?? (this.#open ? this.agentOf(this.#open.chat.agent_id)?.kind : undefined)
    return needsDocker(kind) && this.#engine !== null && !this.#engine.reachable
  }

  /** Run a call that answers with a chat. It never rejects: a failure is kept, to be shown. */
  async #show(call: Promise<Conversation>): Promise<boolean> {
    try {
      this.#install(await call)
      this.#chats = await this.#rpc.chats()
      this.#failure = null
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** Run a call; a failure is kept, to be shown. */
  async #try(call: () => Promise<void>): Promise<boolean> {
    try {
      await call()
      this.#failure = null
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** Read the agents, the chats and Docker, and the open chat again (it may have moved on while nobody was listening). */
  async load(): Promise<void> {
    try {
      ;[this.#agents, this.#chats] = await Promise.all([this.#rpc.agents(), this.#rpc.chats()])
      const last = this.#open?.chat.id
      if (last !== undefined && last !== '' && this.#chats.some((c) => c.id === last)) this.#install(await this.#rpc.open({ chat: last }))
      this.#failure = null
      this.#loaded = true
    } catch (e) {
      this.#failure = messageOf(e)
      return
    }
    await this.#readEngine()
  }

  /** Whether Docker can be reached: an answer, never a failure of the window. */
  async #readEngine(): Promise<void> {
    try {
      const engine = await this.#rpc.engine()
      // the address field starts as what is chosen
      if (this.#engine === null) this.hostDraft = engine.host
      this.#engine = engine
    } catch {
      // Docker matters only to container agents, whose chats say why when one needs it
    }
  }

  /** Use the Docker at the address that is typed; empty is this machine's own. */
  async connect(): Promise<void> {
    await this.#try(async () => {
      this.#engine = await this.#rpc.connect({ host: this.hostDraft.trim() })
      this.hostDraft = this.#engine.host
    })
    // the images are that Docker's
    if (this.#engine?.reachable && this.form?.kind === 'container') await this.#readChoices('container')
  }

  /**
   * Hear the backend while the window is open: what the agent says and does, the end of a turn, and the host
   * connection coming back. Returns a function that stops listening.
   */
  listen(): () => void {
    const stops = [
      this.#events.on('item', (data) => this.#change(data as Changed)),
      this.#events.on('piece', (data) => this.#add(data as Piece)),
      // an item too large to have been sent
      this.#events.on('item_changed', (data) => this.#read(data as Reference)),
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

  /** Show a conversation the backend gave whole: what a read that began before it was made of is older than it. */
  #install(conversation: Conversation | null): void {
    this.#open = conversation
    this.#installed++
  }

  #change(changed: Changed): void {
    // a read that is out is of an older item than this
    const reading = this.#reading.get(readKey(changed.chat, changed.item.id))
    if (reading) reading.again = true
    this.#put(changed)
  }

  #put({ chat, item }: Changed): void {
    const open = this.#open
    if (open?.chat.id !== chat) return
    const known = open.items.some((i) => i.id === item.id)
    // a new item goes where its id puts it: one read later than a newer item that was sent whole still comes before it
    const after = open.items.findIndex((i) => i.id > item.id)
    const items = known
      ? open.items.map((i) => (i.id === item.id ? item : i))
      : after < 0
        ? [...open.items, item]
        : [...open.items.slice(0, after), item, ...open.items.slice(after)]
    // the agent waits while a permission it asked for has no answer
    this.#open = { ...open, items, waiting: items.some((i) => i.kind === 'permission' && !i.done) }
  }

  /**
   * Read an item the backend did not send: it was more than a notification may hold. An item is read one at a time. What
   * a read gives is shown only if nothing newer has come while it was out: a whole conversation installed since is newer
   * than it (the item is read again), and so is a change that came of an item already shown (read again too, so what is
   * shown is never older than the last change). An item not shown yet is shown even then, and read again.
   */
  #read({ chat, item }: Reference): void {
    if (this.#open?.chat.id !== chat) return
    const key = readKey(chat, item)
    const reading = this.#reading.get(key)
    if (reading) {
      reading.again = true
      return
    }
    const state = { again: false }
    this.#reading.set(key, state)
    void (async () => {
      try {
        for (;;) {
          state.again = false
          const installed = this.#installed
          let read: { item: Item } | { failure: unknown }
          try {
            read = { item: await this.#rpc.readItem({ chat, item }) }
          } catch (failure) {
            read = { failure }
          }
          // the chat was left or deleted while the item was read: nothing to say about it
          const open = this.#open
          if (open?.chat.id !== chat) return
          // a whole conversation was installed since the read began: what came (or failed) is older than it
          if (installed !== this.#installed) continue
          if ('failure' in read) {
            this.#failure = messageOf(read.failure)
            return
          }
          const newer = state.again && open.items.some((i) => i.id === read.item.id)
          if (!newer) this.#put({ chat, item: read.item })
          if (!state.again) return
        }
      } finally {
        this.#reading.delete(key)
      }
    })()
  }

  #add(piece: Piece): void {
    const reading = this.#reading.get(readKey(piece.chat, piece.item))
    if (reading) reading.again = true
    const open = this.#open
    if (open?.chat.id !== piece.chat) return
    this.#open = { ...open, items: open.items.map((i) => (i.id === piece.item ? { ...i, text: i.text + piece.text } : i)) }
  }

  /** Close whatever the detail column shows. */
  #clear(): void {
    this.#install(null)
    this.form = null
    this.picking = false
    this.#failure = null
  }

  /**
   * Open a new chat with `agent`. It is the window's own until something is said in it: the backend makes the chat with
   * the first message (`send`), so one left unsaid leaves nothing behind.
   */
  startChat(agent: Agent): void {
    this.#clear()
    this.#open = {
      chat: { id: '', title: 'New chat', agent: agent.name, agent_id: agent.id, updated: '' },
      items: [],
      busy: false,
      waiting: false
    }
    if (needsDocker(agent.kind)) void this.#readEngine()
  }

  openChat(chat: Chat): Promise<boolean> {
    this.form = null
    this.picking = false
    if (needsDocker(this.agentOf(chat.agent_id)?.kind)) void this.#readEngine()
    return this.#show(this.#rpc.open({ chat: chat.id }))
  }

  /** Send what is typed. What was typed comes back when it could not be sent. */
  async send(): Promise<void> {
    if (!this.sendable || this.#open === null) return
    const text = this.draft
    const { id, agent_id } = this.#open.chat
    this.draft = ''
    // a new chat (no id yet) is made by its first message
    const where = id === '' ? { agent: agent_id } : { chat: id }
    if (!(await this.#show(this.#rpc.send({ ...where, text })))) this.draft = text
  }

  /** Stop the agent's answer in the open chat; what it said so far stays. */
  async stop(): Promise<void> {
    if (this.#open !== null) await this.#show(this.#rpc.stop({ chat: this.#open.chat.id }))
  }

  /** Answer a permission the agent asked for with one of the options it offered. */
  async answer(item: Item, option: string): Promise<void> {
    if (this.#open === null) return
    await this.#show(this.#rpc.answer({ chat: this.#open.chat.id, item: item.id, option }))
  }

  /** Delete the open chat (a container chat's container and volume go with it). */
  async removeChat(): Promise<void> {
    this.asking = null
    const open = this.#open
    if (open === null) return
    // a new chat nothing was said in is the window's own: there is nothing to delete
    if (open.chat.id === '') {
      this.#open = null
      return
    }
    if (await this.#try(async () => void (this.#chats = await this.#rpc.removeChat({ chat: open.chat.id })))) this.#open = null
  }

  /**
   * Read what the form of an agent of `kind` offers to choose from: the keychain's items for any (an MCP server's
   * headers), and Docker's images and the Skills app's skills for a container. What could not be read is said in the
   * form, which still takes what is typed.
   */
  async #readChoices(kind: Kind): Promise<void> {
    const reads: Promise<void>[] = [
      this.#rpc.secrets().then(
        (items) => void (this.#secrets = { items, problem: null }),
        (e: unknown) => void (this.#secrets = { items: [], problem: messageOf(e) })
      )
    ]
    if (kind === 'container') {
      reads.push(
        this.#rpc.skills().then(
          ({ skills, problem }) => void (this.#skills = { items: skills, problem: problem || null }),
          (e: unknown) => void (this.#skills = { items: [], problem: messageOf(e) })
        ),
        this.#rpc.images().then(
          ({ images, problem }) => void (this.#images = { items: images, problem: problem || null }),
          (e: unknown) => void (this.#images = { items: [], problem: messageOf(e) })
        ),
        this.#readEngine()
      )
    }
    await Promise.all(reads)
  }

  /** Open the type picker: the first step of adding an agent. */
  add(): void {
    this.#clear()
    this.picking = true
  }

  /**
   * Open the form: to change the agent given, or to add one of `kind` (chosen in the type picker). What the form offers
   * to choose from is read as it opens.
   */
  edit(agent: Agent | null, kind: Kind = agent?.kind ?? 'remote'): Promise<void> {
    this.#failure = null
    this.picking = false
    this.form = agentDraftOf(agent, kind)
    return this.#readChoices(this.form.kind)
  }

  /** The picture of the agent a chat is with; empty when it has none, or was removed. */
  avatarOf(chat: Chat): string {
    return this.agentOf(chat.agent_id)?.avatar ?? ''
  }

  /** Save the form. It stays open, with the reason, when the backend refuses it. */
  async save(): Promise<void> {
    const form = this.form
    if (form === null || !this.savable) return
    const saved = await this.#try(async () => {
      this.#agents =
        form.agent === null ? await this.#rpc.addAgent(addParamsOf(form)) : await this.#rpc.updateAgent(updateParamsOf({ ...form, agent: form.agent }))
      this.#chats = await this.#rpc.chats()
    })
    if (saved) this.form = null
  }

  /**
   * Remove the agent the form is changing. A remote agent's token goes with it; the chats of a remote or a program
   * agent stay, and a container agent's go, with their containers and volumes.
   */
  async remove(): Promise<void> {
    this.asking = null
    const agent = this.form?.agent
    if (!agent) return
    const removed = await this.#try(async () => {
      this.#agents = await this.#rpc.removeAgent({ agent: agent.id })
      this.#chats = await this.#rpc.chats()
    })
    if (removed) this.form = null
  }

  /** Leave the open chat, the type picker or the form for the list (a narrow window shows one or the other). */
  back(): void {
    this.#clear()
  }
}
