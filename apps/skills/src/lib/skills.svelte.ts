import { skillDraftOf, skillParamsOf, type SkillDraft } from './form'

type Skill = VosApp.Skills.Skill

/** What the window needs of the backend: the skills and the ways of changing them (`App.svelte` sends them on the bus). */
export interface SkillsRpc {
  skills(): Promise<Skill[]>
  addSkill(params: VosApp.Skills.AddSkillParams): Promise<Skill[]>
  updateSkill(params: VosApp.Skills.UpdateSkillParams): Promise<Skill[]>
  removeSkill(params: VosApp.Skills.RemoveSkillParams): Promise<Skill[]>
}
/** Where the window hears that the host connection came back (`host:reconnected`): what changed while it was away reached nobody. */
export interface SkillsEvents {
  on(name: 'host:reconnected', fn: () => void): () => void
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's Skills: the skills, and the form one is added or changed in while it is open. The skills are the
 * backend's: every change is a call that answers with all of them, and this holds the last answer. The form is the
 * window's own until it is saved.
 */
export class Skills {
  // `raw`: the list is replaced whole by each answer, never edited in place
  #skills = $state.raw<Skill[]>([])
  #loaded = $state(false)
  // why the last call failed, in the backend's words
  #failure = $state<string | null>(null)
  /** The skill form, while it is open. */
  form = $state<SkillDraft | null>(null)
  /** The person asked to remove the skill the form changes, and has not said yes or no. */
  asking = $state(false)

  readonly #rpc: SkillsRpc
  readonly #events: SkillsEvents

  constructor(rpc: SkillsRpc, events: SkillsEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  /** The skills, by name. */
  get skills(): readonly Skill[] {
    return this.#skills
  }
  /** The first answer has arrived. */
  get loaded(): boolean {
    return this.#loaded
  }
  get failure(): string | null {
    return this.#failure
  }
  /** Whether the form can be saved: a name and its SKILL.md. */
  get savable(): boolean {
    return this.form !== null && this.form.name.trim() !== '' && this.form.text.trim() !== ''
  }

  /** Run a call that answers with the skills. It never rejects: a failure is kept, to be shown. */
  async #take(call: () => Promise<Skill[]>): Promise<boolean> {
    try {
      this.#skills = await call()
      this.#loaded = true
      this.#failure = null
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** Read the skills. */
  async load(): Promise<void> {
    await this.#take(() => this.#rpc.skills())
  }

  /** Read the skills again when the host connection comes back. Returns a function that stops listening. */
  listen(): () => void {
    return this.#events.on('host:reconnected', () => void this.load())
  }

  /** Open the form: to add a skill, or to change the one given. */
  edit(skill: Skill | null): void {
    this.form = skillDraftOf(skill)
    this.asking = false
    this.#failure = null
  }

  /** Leave the form for the list, keeping nothing of it. */
  close(): void {
    this.form = null
    this.asking = false
    this.#failure = null
  }

  /** Save the form. It stays open, with the reason, when the backend refuses it. */
  async save(): Promise<boolean> {
    const form = this.form
    if (form === null || !this.savable) return false
    const params = skillParamsOf(form)
    const saved = await this.#take(() => (form.skill === null ? this.#rpc.addSkill(params) : this.#rpc.updateSkill({ skill: form.skill.id, ...params })))
    if (saved && this.form === form) this.form = null
    return saved
  }

  /** The answer was Remove: the skill the form changes goes. */
  async remove(): Promise<boolean> {
    this.asking = false
    const skill = this.form?.skill
    if (!skill) return false
    const removed = await this.#take(() => this.#rpc.removeSkill({ skill: skill.id }))
    if (removed) this.form = null
    return removed
  }
}
