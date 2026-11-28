import { copilotDraftOf, copilotParamsOf, skillDraftOf, skillParamsOf, type CopilotDraft, type SkillDraft } from './form'

type BuildStatus = VosApp.Copilot.BuildStatus
type Chat = VosApp.Copilot.Chat
type Conversation = VosApp.Copilot.Conversation
type Copilot = VosApp.Copilot.Copilot
type Engine = VosApp.Copilot.Engine
type Item = VosApp.Copilot.Item
type Secret = VosApp.Copilot.Secret
type Skill = VosApp.Copilot.Skill

/**
 * What the window needs of the backend: Docker, the copilots, the skill library, the keychain's items, and the chats
 * (the app's private methods on the bus, `_engine` ...: `App.svelte` sends them).
 */
export interface CopilotRpc {
  engine(): Promise<Engine>
  connect(params: VosApp.Copilot.ConnectParams): Promise<Engine>
  copilots(): Promise<Copilot[]>
  addCopilot(params: VosApp.Copilot.AddCopilotParams): Promise<Copilot[]>
  updateCopilot(params: VosApp.Copilot.UpdateCopilotParams): Promise<Copilot[]>
  removeCopilot(params: VosApp.Copilot.RemoveCopilotParams): Promise<Copilot[]>
  build(params: VosApp.Copilot.BuildParams): Promise<Copilot[]>
  skills(): Promise<Skill[]>
  addSkill(params: VosApp.Copilot.AddSkillParams): Promise<Skill[]>
  updateSkill(params: VosApp.Copilot.UpdateSkillParams): Promise<Skill[]>
  removeSkill(params: VosApp.Copilot.RemoveSkillParams): Promise<Skill[]>
  secrets(): Promise<Secret[]>
  chats(): Promise<Chat[]>
  open(params: VosApp.Copilot.OpenParams): Promise<Conversation>
  send(params: VosApp.Copilot.SendParams): Promise<Conversation>
  answer(params: VosApp.Copilot.AnswerParams): Promise<Conversation>
  stop(params: VosApp.Copilot.StopParams): Promise<Conversation>
  removeChat(params: VosApp.Copilot.RemoveChatParams): Promise<Chat[]>
}
/**
 * Where the window hears the backend's notifications (`item`, `piece`, `turn`, `building`: `App.svelte` wires them to
 * the bus) and the host's events (`host:reconnected`).
 */
export interface CopilotEvents {
  on(name: string, fn: (data: unknown) => void): () => void
}

/** How the last build of a copilot's image that this window heard of goes: its lines so far, and whether it is over. */
export interface Build {
  lines: string[]
  status: BuildStatus
}

/** What the detail column shows besides a chat. */
export type Pane = 'copilot' | 'skills' | 'skill' | null

/** What the person asked to delete, until they say yes or no. */
export type Asking = 'chat' | 'copilot' | 'skill' | null

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's Copilot: whether Docker can be reached, the copilots and the skill library, the chats, and what the
 * detail column shows (a chat, a copilot's form, the library or a skill's form). The copilots, skills and chats are
 * the backend's; this holds what the window shows of them, and what is being typed.
 */
export class Copilots {
  #engine = $state.raw<Engine | null>(null)
  #copilots = $state.raw<Copilot[]>([])
  #skills = $state.raw<Skill[]>([])
  #chats = $state.raw<Chat[]>([])
  // the chat that is open: replaced whole by each answer of the backend, and changed by its events in between
  #open = $state.raw<Conversation | null>(null)
  #failure = $state<string | null>(null)
  #loaded = $state(false)
  #secrets = $state.raw<Secret[]>([])
  // why the keychain's items could not be read; the form still takes values written out
  #secretsProblem = $state<string | null>(null)
  #builds = $state.raw<Readonly<Record<string, Build>>>({})
  /** The address typed for another Docker. */
  hostDraft = $state('')
  /** What is being typed in the open chat. */
  draft = $state('')
  /** The copilot form, while it is open. */
  form = $state<CopilotDraft | null>(null)
  /** The skill form, while it is open. */
  skillForm = $state<SkillDraft | null>(null)
  /** The skill library is open. */
  library = $state(false)
  asking = $state<Asking>(null)

  readonly #rpc: CopilotRpc
  readonly #events: CopilotEvents

  constructor(rpc: CopilotRpc, events: CopilotEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  get engine(): Engine | null {
    return this.#engine
  }
  get copilots(): readonly Copilot[] {
    return this.#copilots
  }
  get skills(): readonly Skill[] {
    return this.#skills
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
  /** Everything has been read once: an empty list is empty, not still coming. */
  get loaded(): boolean {
    return this.#loaded
  }
  /** The keychain's items a variable or a header can take its value from. */
  get secrets(): readonly Secret[] {
    return this.#secrets
  }
  get secretsProblem(): string | null {
    return this.#secretsProblem
  }
  /** What the detail column shows besides a chat; null for a chat, or nothing. */
  get pane(): Pane {
    return this.skillForm ? 'skill' : this.library ? 'skills' : this.form ? 'copilot' : null
  }
  /** Whether what is typed can be sent: there is something to say, and the agent is not answering. */
  get sendable(): boolean {
    return this.#open !== null && !this.#open.busy && this.draft.trim() !== ''
  }
  /** Whether the copilot form can be saved: a name, a Dockerfile and a command. What is wrong with them is the backend's to say. */
  get savable(): boolean {
    const form = this.form
    return form !== null && form.name.trim() !== '' && form.dockerfile.trim() !== '' && form.command.trim() !== ''
  }
  /** Whether the skill form can be saved: a name and its SKILL.md. */
  get skillSavable(): boolean {
    return this.skillForm !== null && this.skillForm.name.trim() !== '' && this.skillForm.text.trim() !== ''
  }

  /** The copilot the form changes, as the backend has it now (its image, its build); null for a new one. */
  get editing(): Copilot | null {
    const id = this.form?.copilot?.id
    return this.#copilots.find((c) => c.id === id) ?? null
  }

  /** The build of a copilot's image this window heard of last. */
  buildOf(copilot: string): Build | null {
    return this.#builds[copilot] ?? null
  }

  /** The chats with a copilot, the most recently used first. */
  chatsOf(copilot: string): Chat[] {
    return this.#chats.filter((chat) => chat.copilot_id === copilot)
  }

  /** Close whatever the detail column shows. */
  #clear(): void {
    this.#open = null
    this.form = null
    this.skillForm = null
    this.library = false
    this.#failure = null
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

  /** Read Docker, the copilots, the skills and the chats, and the open chat again (it may have moved on while nobody was listening). */
  async load(): Promise<void> {
    await this.#try(async () => {
      const engine = await this.#rpc.engine()
      ;[this.#copilots, this.#skills, this.#chats] = await Promise.all([this.#rpc.copilots(), this.#rpc.skills(), this.#rpc.chats()])
      // the address field starts as what is chosen
      if (this.#engine === null) this.hostDraft = engine.host
      this.#engine = engine
      const last = this.#open?.chat.id
      if (last && this.#chats.some((c) => c.id === last)) this.#open = await this.#rpc.open({ chat: last })
      this.#loaded = true
    })
  }

  /** Use the Docker at the address that is typed; empty is this machine's own. */
  async connect(): Promise<void> {
    await this.#try(async () => {
      this.#engine = await this.#rpc.connect({ host: this.hostDraft.trim() })
      this.hostDraft = this.#engine.host
    })
  }

  /**
   * Hear the backend while the window is open: what the agent says and does, the end of a turn, how a build goes, and
   * the host connection coming back. Returns a function that stops listening.
   */
  listen(): () => void {
    const stops = [
      this.#events.on('item', (data) => this.#change(data as VosApp.Copilot.ItemParams)),
      this.#events.on('piece', (data) => this.#add(data as VosApp.Copilot.PieceParams)),
      // the turn is over (complete, stopped or failed): read the chat as the backend has it
      this.#events.on('turn', (data) => {
        const chat = (data as VosApp.Copilot.TurnParams).chat
        if (this.#open?.chat.id === chat) void this.#show(this.#rpc.open({ chat }))
      }),
      this.#events.on('building', (data) => this.#building(data as VosApp.Copilot.BuildingParams)),
      // events sent while the connection was away reached nobody
      this.#events.on('host:reconnected', () => void this.load())
    ]
    return () => stops.forEach((stop) => stop())
  }

  #change({ chat, item }: VosApp.Copilot.ItemParams): void {
    const open = this.#open
    if (open?.chat.id !== chat) return
    const known = open.items.some((i) => i.id === item.id)
    const items = known ? open.items.map((i) => (i.id === item.id ? item : i)) : [...open.items, item]
    // the agent waits while a permission it asked for has no answer
    this.#open = { ...open, items, waiting: items.some((i) => i.kind === 'permission' && !i.done) }
  }

  #add(piece: VosApp.Copilot.PieceParams): void {
    const open = this.#open
    if (open?.chat.id !== piece.chat) return
    this.#open = { ...open, items: open.items.map((i) => (i.id === piece.item ? { ...i, text: i.text + piece.text } : i)) }
  }

  #building({ copilot, line, status }: VosApp.Copilot.BuildingParams): void {
    const before = this.#builds[copilot]
    // a build that was over is followed by a new one
    const lines = before && before.status === 'running' ? before.lines : []
    this.#builds = { ...this.#builds, [copilot]: { lines: [...lines, line], status } }
    // once it is over, the copilot has a new image, or a reason it has none
    if (status !== 'running') void this.#try(async () => void (this.#copilots = await this.#rpc.copilots()))
  }

  /**
   * Open a new chat with `copilot`. It is the window's own until something is said in it: the backend makes the chat
   * with the first message (`send`), so one left unsaid leaves nothing behind.
   */
  startChat(copilot: Copilot): void {
    this.#clear()
    this.#open = {
      chat: { id: '', title: 'New chat', copilot: copilot.name, copilot_id: copilot.id, updated: '' },
      items: [],
      busy: false,
      waiting: false
    }
  }

  openChat(chat: Chat): Promise<boolean> {
    this.#clear()
    return this.#show(this.#rpc.open({ chat: chat.id }))
  }

  /** Send what is typed. What was typed comes back when it could not be sent. */
  async send(): Promise<void> {
    if (!this.sendable || this.#open === null) return
    const text = this.draft
    const { id, copilot_id } = this.#open.chat
    this.draft = ''
    // a new chat (no id yet) is made by its first message
    const where = id === '' ? { copilot: copilot_id } : { chat: id }
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

  /** Delete the open chat, with its container and volume. */
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

  /** Read the keychain's items for the form's pickers. */
  async #readSecrets(): Promise<void> {
    try {
      this.#secrets = await this.#rpc.secrets()
      this.#secretsProblem = null
    } catch (e) {
      this.#secrets = []
      this.#secretsProblem = messageOf(e)
    }
  }

  /** Open the form: to add a copilot, or to change the one given. */
  editCopilot(copilot: Copilot | null): void {
    this.#clear()
    this.form = copilotDraftOf(copilot)
    void this.#readSecrets()
  }

  /** Send the form; answers with the copilot as saved, or undefined when the backend refused it (the reason is kept). */
  async #saveCopilot(): Promise<Copilot | undefined> {
    const form = this.form
    if (form === null || !this.savable) return undefined
    const params = copilotParamsOf(form)
    let saved: Copilot | undefined
    const done = await this.#try(async () => {
      const before = new Set(this.#copilots.map((c) => c.id))
      this.#copilots = form.copilot === null ? await this.#rpc.addCopilot(params) : await this.#rpc.updateCopilot({ copilot: form.copilot.id, ...params })
      saved = this.#copilots.find((c) => (form.copilot === null ? !before.has(c.id) : c.id === form.copilot.id))
      this.#chats = await this.#rpc.chats()
    })
    return done ? saved : undefined
  }

  /** Save the form, and close it. It stays open, with the reason, when the backend refuses it. */
  async saveCopilot(): Promise<void> {
    if (await this.#saveCopilot()) this.form = null
  }

  /** Save the form, which stays open on the copilot as saved, and build its image from the Dockerfile. */
  async build(): Promise<void> {
    const saved = await this.#saveCopilot()
    if (!saved || this.form === null) return
    this.form = { ...this.form, copilot: saved }
    this.#builds = { ...this.#builds, [saved.id]: { lines: [], status: 'running' } }
    await this.#try(async () => void (this.#copilots = await this.#rpc.build({ copilot: saved.id })))
  }

  /** Remove the copilot the form is changing, with its chats, their containers and volumes, and its images. */
  async removeCopilot(): Promise<void> {
    this.asking = null
    const copilot = this.form?.copilot
    if (!copilot) return
    const removed = await this.#try(async () => {
      this.#copilots = await this.#rpc.removeCopilot({ copilot: copilot.id })
      this.#chats = await this.#rpc.chats()
    })
    if (removed) this.form = null
  }

  /** Open the skill library. */
  openLibrary(): void {
    this.#clear()
    this.library = true
  }

  /** Open the skill form: to add a skill, or to change the one given. It goes back to the library when it is done. */
  editSkill(skill: Skill | null): void {
    this.#clear()
    this.library = true
    this.skillForm = skillDraftOf(skill)
  }

  /** Leave the skill form for the library. */
  closeSkill(): void {
    this.skillForm = null
    this.#failure = null
  }

  /** Save the skill form. It stays open, with the reason, when the backend refuses it. */
  async saveSkill(): Promise<void> {
    const form = this.skillForm
    if (form === null || !this.skillSavable) return
    const params = skillParamsOf(form)
    const saved = await this.#try(async () => {
      this.#skills = form.skill === null ? await this.#rpc.addSkill(params) : await this.#rpc.updateSkill({ skill: form.skill.id, ...params })
    })
    if (saved) this.skillForm = null
  }

  /** Remove the skill the form is changing, from the library and from the copilots that use it. */
  async removeSkill(): Promise<void> {
    this.asking = null
    const skill = this.skillForm?.skill
    if (!skill) return
    const removed = await this.#try(async () => {
      this.#skills = await this.#rpc.removeSkill({ skill: skill.id })
      this.#copilots = await this.#rpc.copilots()
    })
    if (removed) this.skillForm = null
  }

  /** Leave what the detail column shows for the list (a narrow window shows one or the other). */
  back(): void {
    this.#clear()
  }
}
