import type { Api, Issue, IssueType, Page, Priority, Project, Status, Workspace } from '@projects/rpc'
import type { Host } from '@vos/host'
import { COLORS, column, suggestKey, tree, type Leaf, type Tab } from './catalog'
import { keyProblem, nameProblem } from './validate'

/** What the window needs of the backend: the workspace, a page's text, and each way of changing them. */
export type ProjectsRpc = Pick<
  Api,
  'workspace' | 'addProject' | 'removeProject' | 'addIssue' | 'updateIssue' | 'removeIssue' | 'addPage' | 'readPage' | 'writePage' | 'removePage'
>
/** Where the backend's events are heard (`host.on`). */
export type ProjectsEvents = Pick<Host, 'on'>

/** A new project as it is typed. */
export interface ProjectDraft {
  name: string
  key: string
}
/** What is wrong with each field of a form, null where nothing is. */
export type Problems<Field extends string> = Readonly<Record<Field, string | null>>
/** What can change on an issue: only what is given changes. */
export interface IssueChanges {
  title?: string
  description?: string
  type?: IssueType
  priority?: Priority
  status?: Status
}
/** A page as it is being rewritten. */
export interface PageDraft {
  title: string
  body: string
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
/** The backend refused what it was sent (`ctx:fail`), as opposed to not being reached. */
const isRefusal = (e: unknown): boolean => (e as { code?: unknown } | null)?.code === -32010

/**
 * One window's Projects. The projects, their issues and their pages are the backend's: every change is a call that
 * answers with the whole workspace, and this holds the last one. What is the window's own is here: which project is
 * shown and which part of it, the issue or the page that is open, and what is being typed.
 */
export class Projects {
  // `raw`: the workspace is replaced whole by each answer, never edited in place
  #workspace = $state.raw<Workspace>({ projects: [], issues: [], pages: [] })
  #loaded = $state(false)
  // why the last call failed: the backend's own message, or the host's. It stays until a call works
  #failure = $state<string | null>(null)
  #selected = $state<number | null>(null)
  // a window too narrow for both shows the projects until one is picked
  #browsing = $state(true)
  #tab = $state<Tab>('board')
  #issue = $state<number | null>(null)
  #page = $state<number | null>(null)
  // the text of the open page, once it has been read
  #body = $state<string | null>(null)
  #edit = $state<PageDraft | null>(null)
  #form = $state<ProjectDraft | null>(null)
  // the key was typed by the person: it no longer follows the name
  #keyed = false
  // what the backend refused of the project that was sent (its key is taken): it goes when the form is typed in again
  #refused = $state<string | null>(null)
  #busy = $state(false)
  /** What is being typed as the title of the next issue. */
  issueDraft = $state('')
  /** What is being typed as the title of the next page. */
  pageDraft = $state('')

  readonly #rpc: ProjectsRpc
  readonly #events: ProjectsEvents

  constructor(rpc: ProjectsRpc, events: ProjectsEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  get projects(): readonly Project[] {
    return this.#workspace.projects
  }
  /** Whether the workspace has been read once: until then there is nothing to say about it. */
  get loaded(): boolean {
    return this.#loaded
  }
  get failure(): string | null {
    return this.#failure
  }
  /** The projects are in front, not one of them (it matters only in a window too narrow for both). */
  get browsing(): boolean {
    return this.#browsing
  }
  get tab(): Tab {
    return this.#tab
  }
  /** The new project as it is typed, while one is being added. */
  get form(): ProjectDraft | null {
    return this.#form
  }
  get refused(): string | null {
    return this.#refused
  }
  /** A project is being added: nothing more is sent until that is answered. */
  get busy(): boolean {
    return this.#busy
  }
  /** The text of the open page; null until it has been read. */
  get body(): string | null {
    return this.#body
  }
  /** The open page as it is being rewritten, while it is. */
  get edit(): PageDraft | null {
    return this.#edit
  }

  /** The project that is shown. */
  readonly project: Project | undefined = $derived(this.#workspace.projects.find((p) => p.id === this.#selected))
  /** The issues of the project that is shown, in the order they were added. */
  readonly issues: Issue[] = $derived(this.#workspace.issues.filter((i) => i.project === this.#selected))
  /** The board of the project that is shown: its issues by column, the most pressing first. */
  readonly columns: Readonly<Record<Status, Issue[]>> = $derived({
    todo: column(this.issues, 'todo'),
    doing: column(this.issues, 'doing'),
    done: column(this.issues, 'done')
  })
  /** The pages of the project that is shown, each followed by the pages under it. */
  readonly pages: Leaf[] = $derived(tree(this.#workspace.pages.filter((p) => p.project === this.#selected)))
  /** The issue that is open. */
  readonly issue: Issue | undefined = $derived(this.#workspace.issues.find((i) => i.id === this.#issue))
  /** The page that is open. */
  readonly page: Leaf | undefined = $derived(this.pages.find((l) => l.page.id === this.#page))
  /** The pages directly under the open page. */
  readonly subpages: Page[] = $derived(this.pages.filter((l) => l.page.parent === this.#page).map((l) => l.page))
  /** How many issues are still open in each project, by the project's id. */
  readonly open: ReadonlyMap<number, number> = $derived.by(() => {
    const left = new Map<number, number>()
    for (const issue of this.#workspace.issues) if (issue.status !== 'done') left.set(issue.project, (left.get(issue.project) ?? 0) + 1)
    return left
  })
  /** What is wrong with the new project as it is typed. */
  readonly problems: Problems<'name' | 'key'> = $derived({
    name: nameProblem(this.#form?.name ?? ''),
    key: keyProblem(this.#form?.key ?? '')
  })

  /** Take a workspace as what the backend has, and leave what is no longer in it. */
  #take(workspace: Workspace): void {
    this.#workspace = workspace
    this.#loaded = true
    this.#failure = null
    if (!workspace.projects.some((p) => p.id === this.#selected)) {
      // the project that was shown is gone (deleted here, or in another window), or none was picked yet
      this.#selected = workspace.projects[0]?.id ?? null
      this.#issue = null
      this.#closePage()
    }
    if (this.#issue !== null && !workspace.issues.some((i) => i.id === this.#issue)) this.#issue = null
    if (this.#page !== null && !workspace.pages.some((p) => p.id === this.#page)) this.#closePage()
  }

  /**
   * Run a call that answers with the workspace. It never rejects: a failure is kept, to be shown. What the backend
   * refused of a form goes to `refused`, to be said at the form; everything else is the window's.
   */
  async #show(call: Promise<Workspace>, refused?: (message: string) => void): Promise<boolean> {
    try {
      this.#take(await call)
      return true
    } catch (e) {
      if (refused && isRefusal(e)) refused(messageOf(e))
      else this.#failure = messageOf(e)
      return false
    }
  }

  #closePage(): void {
    this.#page = null
    this.#body = null
    this.#edit = null
  }

  /** Read the text of the open page. An answer for a page that has been left since is dropped. */
  async #read(id: number): Promise<void> {
    try {
      const text = await this.#rpc.readPage({ page: id })
      if (this.#page !== id) return
      this.#body = text.body
      this.#failure = null
    } catch (e) {
      if (this.#page === id) this.#failure = messageOf(e)
    }
  }

  /** Read the workspace, and the open page again unless it is being rewritten (what is typed is the person's). */
  async load(): Promise<void> {
    await this.#show(this.#rpc.workspace())
    if (this.#page !== null && this.#edit === null) await this.#read(this.#page)
  }

  /**
   * Hear the backend while the window is open: an agent changed something, or the host connection came back (a change
   * made while it was away reached nobody). Returns a function that stops listening.
   */
  listen(): () => void {
    const stops = [
      this.#events.on('projects.changed', () => void this.load()),
      this.#events.on('host:reconnected', () => void this.load())
    ]
    return () => stops.forEach((stop) => stop())
  }

  /** Show a project, on the part of it that was last shown. */
  select(project: Project): void {
    this.#selected = project.id
    this.#browsing = false
    this.#form = null
    this.#issue = null
    this.#closePage()
  }

  /** Leave the project for the projects (a narrow window shows one or the other). */
  back(): void {
    this.#browsing = true
  }

  /** Show the board, the issues or the pages of the project. */
  show(tab: Tab): void {
    this.#tab = tab
  }

  /** Start a new project: an empty form, in front. */
  begin(): void {
    this.#form = { name: '', key: '' }
    this.#keyed = false
    this.#refused = null
    this.#browsing = false
  }

  /** Give the new project up. A narrow window with no project to go back to shows the projects. */
  cancel(): void {
    this.#form = null
    this.#refused = null
    if (this.project === undefined) this.#browsing = true
  }

  /** Something was typed in the new project. Until a key is typed, the key follows the name. */
  draft(change: Partial<ProjectDraft>): void {
    if (this.#form === null) return
    if (change.key !== undefined) this.#keyed = change.key.trim() !== ''
    const name = change.name ?? this.#form.name
    const key = (change.key ?? this.#form.key).toUpperCase()
    this.#form = { name, key: this.#keyed ? key : suggestKey(name) }
    this.#refused = null
  }

  /** Whether the new project can be sent: there is a name and a key, and nothing wrong with either. */
  readonly ready: boolean = $derived(
    this.#form !== null &&
      this.#form.name.trim() !== '' &&
      this.#form.key.trim() !== '' &&
      this.problems.name === null &&
      this.problems.key === null
  )

  /** Add the project that is typed, in the next colour, and show it. The form stays when it could not be added. */
  async addProject(): Promise<void> {
    const form = this.#form
    if (form === null || !this.ready || this.#busy) return
    const key = form.key.trim().toUpperCase()
    const color = COLORS[this.#workspace.projects.length % COLORS.length] ?? 'blue'
    this.#busy = true
    const added = await this.#show(this.#rpc.addProject({ name: form.name.trim(), key, color }), (message) => (this.#refused = message))
    this.#busy = false
    if (!added) return
    const project = this.#workspace.projects.find((p) => p.key === key)
    this.#form = null
    if (project) this.select(project)
  }

  /** Delete a project, with its issues and its pages. The window goes on to another, and to the projects when narrow. */
  async removeProject(project: Project): Promise<void> {
    const shown = this.#selected === project.id
    if ((await this.#show(this.#rpc.removeProject({ project: project.id }))) && shown) this.#browsing = true
  }

  /** Add what is typed as an issue of the project that is shown. What was typed comes back when it could not be. */
  async addIssue(): Promise<void> {
    const title = this.issueDraft.trim()
    const project = this.project
    if (title === '' || project === undefined) return
    const typed = this.issueDraft
    this.issueDraft = ''
    if (!(await this.#show(this.#rpc.addIssue({ project: project.id, title })))) this.issueDraft = typed
  }

  /** Open an issue, to read and change it. */
  openIssue(issue: Issue): void {
    this.#issue = issue.id
  }

  /** Leave the open issue for the board or the list it was opened from. */
  closeIssue(): void {
    this.#issue = null
  }

  /** Change an issue. Nothing is sent when nothing would change, or for a blank title. */
  async changeIssue(issue: Issue, changes: IssueChanges): Promise<void> {
    const next: IssueChanges = {}
    const title = changes.title?.trim()
    if (title !== undefined && title !== '' && title !== issue.title) next.title = title
    if (changes.description !== undefined && changes.description !== issue.description) next.description = changes.description
    if (changes.type !== undefined && changes.type !== issue.type) next.type = changes.type
    if (changes.priority !== undefined && changes.priority !== issue.priority) next.priority = changes.priority
    if (changes.status !== undefined && changes.status !== issue.status) next.status = changes.status
    if (Object.keys(next).length > 0) await this.#show(this.#rpc.updateIssue({ issue: issue.id, ...next }))
  }

  /** Move an issue to another column of the board. */
  move(issue: Issue, status: Status): Promise<void> {
    return this.changeIssue(issue, { status })
  }

  removeIssue(issue: Issue): Promise<boolean> {
    return this.#show(this.#rpc.removeIssue({ issue: issue.id }))
  }

  /**
   * Add what is typed as a page of the project that is shown, under the open page when one is, and open it to be
   * written. What was typed comes back when it could not be added.
   */
  async addPage(): Promise<void> {
    const title = this.pageDraft.trim()
    const project = this.project
    if (title === '' || project === undefined) return
    const typed = this.pageDraft
    const before = new Set(this.#workspace.pages.map((p) => p.id))
    this.pageDraft = ''
    if (!(await this.#show(this.#rpc.addPage({ project: project.id, title, ...(this.#page !== null ? { parent: this.#page } : {}) })))) {
      this.pageDraft = typed
      return
    }
    const added = this.#workspace.pages.find((p) => !before.has(p.id))
    if (!added) return
    // a new page has no text yet: it opens ready to be written
    this.#page = added.id
    this.#body = ''
    this.#edit = { title: added.title, body: '' }
  }

  /** Open a page, and read its text. */
  async openPage(page: Page): Promise<void> {
    this.#page = page.id
    this.#body = null
    this.#edit = null
    await this.#read(page.id)
  }

  /** Leave the open page: for the page it is under, or for the list of pages. What was being typed is dropped. */
  async closePage(): Promise<void> {
    const parent = this.pages.find((l) => l.page.id === this.page?.page.parent)
    if (parent) await this.openPage(parent.page)
    else this.#closePage()
  }

  /** Start rewriting the open page, once its text has been read. */
  rewrite(): void {
    const leaf = this.page
    if (leaf && this.#body !== null) this.#edit = { title: leaf.page.title, body: this.#body }
  }

  /** Something was typed in the open page. */
  type(change: Partial<PageDraft>): void {
    if (this.#edit !== null) this.#edit = { ...this.#edit, ...change }
  }

  /** Drop what was typed in the open page. */
  discard(): void {
    this.#edit = null
  }

  /** Keep what was typed in the open page. It stays open to be typed in when it could not be kept, or has no title. */
  async save(): Promise<void> {
    const leaf = this.page
    const edit = this.#edit
    if (!leaf || edit === null || edit.title.trim() === '') return
    if (edit.title.trim() === leaf.page.title && edit.body === this.#body) {
      this.#edit = null
      return
    }
    if (!(await this.#show(this.#rpc.writePage({ page: leaf.page.id, title: edit.title.trim(), body: edit.body })))) return
    if (this.#page === leaf.page.id) {
      this.#body = edit.body
      this.#edit = null
    }
  }

  /** Delete a page and the pages under it. The window goes on to the page it was under, or to the list of pages. */
  async removePage(page: Page): Promise<void> {
    const parent = this.#workspace.pages.find((p) => p.id === page.parent)
    if (!(await this.#show(this.#rpc.removePage({ page: page.id })))) return
    if (parent && this.#page === null) await this.openPage(parent)
  }
}
