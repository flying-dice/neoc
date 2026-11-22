import type { Api, Backlink, Hit, Host, Library, Note, Notebook, Opened, Summary } from 'app:rpc'
import { dayOf } from './dates'
import type { GraphLink, GraphNode } from './graph'
import { listed, named, tagCounts, type TagCount } from './library'
import { keyOf, titleProblem, toggleTask } from './wiki'

/** What the window needs of the backend: the library, a note, and each way of changing them. */
export type NotebooksRpc = Pick<
  Api,
  'library' | 'read' | 'create' | 'follow' | 'save' | 'rename' | 'pin' | 'remove' | 'search' | 'addNotebook' | 'removeNotebook'
>
/** Where the backend's events are heard (`host.on`). */
export type NotebooksEvents = Pick<Host, 'on'>

/** How the open note is shown: its Markdown as it is written, or as it reads. */
export type Mode = 'edit' | 'read'
/** What is beside the list of notes: the open note, or the graph of the notebook. */
export type Pane = 'note' | 'graph'
/** Whether the backend has what the note says: it does, it is being sent, or it is still to be sent. */
export type Status = 'saved' | 'saving' | 'edited'

export interface Options {
  /** The person's clock: it names today's note. */
  now?: () => Date
  /** How long after the last keystroke the note is saved, in ms; null to save only when asked (`flush`). */
  autosave?: number | null
}

/** The code of a call the backend refused with a sentence of its own (`ctx:fail`). */
const REFUSED = -32010
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
/** The backend's own sentence when it refused the call; null for any other failure (the host, the connection). */
const refusalOf = (e: unknown): string | null => (e instanceof Error && 'code' in e && e.code === REFUSED ? e.message : null)

const EMPTY: Library = { notebooks: [], notes: [], links: [] }

/**
 * One window's Notebooks. The notebooks and the notes are the backend's: every change is a call that answers with the
 * library, and this holds the last one, with the note that is open. What is the window's own is here: which notebook
 * and which note are shown, the text being written (sent a moment after the last keystroke), what is searched for, and
 * what is being typed as a name.
 */
export class Notebooks {
  // `raw`: the library and the note are replaced whole by each answer, never edited in place
  #library = $state.raw<Library>(EMPTY)
  #note = $state.raw<Note | null>(null)
  #notebook = $state<number | null>(null)
  // why the last call failed, when it was not about what was typed: the host's message. It stays until a call works
  #failure = $state<string | null>(null)
  #mode = $state<Mode>('edit')
  #pane = $state<Pane>('note')
  // a window too narrow for both shows the list of notes until one is picked
  #browsing = $state(true)
  #tag = $state<string | null>(null)
  #query = $state('')
  #hits = $state.raw<readonly Hit[] | null>(null)
  // the text the backend has of the open note
  #saved = $state('')
  #saving = $state(false)
  #titleProblem = $state<string | null>(null)
  #notebookProblem = $state<string | null>(null)
  #searches = 0
  #timer: ReturnType<typeof setTimeout> | undefined

  /** The open note's text as it is being written. */
  body = $state('')
  /** What is being typed as the name of the next notebook. */
  notebookDraft = $state('')
  /** The quick switcher is open. */
  switching = $state(false)

  readonly #rpc: NotebooksRpc
  readonly #events: NotebooksEvents
  readonly #now: () => Date
  readonly #autosave: number | null

  constructor(rpc: NotebooksRpc, events: NotebooksEvents, options: Options = {}) {
    this.#rpc = rpc
    this.#events = events
    this.#now = options.now ?? (() => new Date())
    this.#autosave = options.autosave === undefined ? 800 : options.autosave
  }

  get notebooks(): readonly Notebook[] {
    return this.#library.notebooks
  }
  /** The open note, as the backend last sent it; its text as it is being written is `body`. */
  get note(): Note | null {
    return this.#note
  }
  get failure(): string | null {
    return this.#failure
  }
  get mode(): Mode {
    return this.#mode
  }
  get pane(): Pane {
    return this.#pane
  }
  /** The list of notes is in front, not a note (it matters only in a window too narrow for both). */
  get browsing(): boolean {
    return this.#browsing
  }
  /** The tag the list is narrowed to. */
  get tag(): string | null {
    return this.#tag
  }
  /** The words that are searched for. */
  get query(): string {
    return this.#query
  }
  /** What the search found; null while nothing is searched for. */
  get hits(): readonly Hit[] | null {
    return this.#hits
  }
  /** What is wrong with the title that was typed for the open note. */
  get titleProblem(): string | null {
    return this.#titleProblem
  }
  /** What is wrong with the name that was typed for a notebook. */
  get notebookProblem(): string | null {
    return this.#notebookProblem
  }
  /** The moment on the person's clock: what "today" is read from. */
  get now(): Date {
    return this.#now()
  }

  /** The notebook that is shown. */
  readonly notebook: Notebook | undefined = $derived(this.#library.notebooks.find((n) => n.id === this.#notebook))
  /** Every note of the notebook that is shown, the most recently changed first. */
  readonly all: Summary[] = $derived(listed(this.#library.notes, this.#notebook, null))
  /** The notes the sidebar lists: the bookmarked ones first, and only the ones with the tag when there is one. */
  readonly notes: Summary[] = $derived(listed(this.#library.notes, this.#notebook, this.#tag))
  /** The tags of the notebook's notes, with how many notes have each. */
  readonly tags: TagCount[] = $derived(tagCounts(this.#library.notes, this.#notebook))
  readonly backlinks: readonly Backlink[] = $derived(this.#note?.backlinks ?? [])
  readonly status: Status = $derived(this.#saving ? 'saving' : this.body === this.#saved ? 'saved' : 'edited')
  /** The notes of the notebook and the links between them, for its graph. */
  readonly graph: { nodes: GraphNode[]; links: GraphLink[] } = $derived.by(() => {
    const nodes = this.all.map((n) => ({ id: n.id, title: n.title }))
    const inside = new Set(nodes.map((n) => n.id))
    return { nodes, links: this.#library.links.filter((l) => inside.has(l.from) && inside.has(l.to)) }
  })

  /** Whether a link's title names a note of the notebook that is shown: a link to no note is drawn fainter. */
  resolves(title: string): boolean {
    return named(this.#library.notes, this.#notebook, title) !== undefined
  }

  /** Keep a library the backend sent, and let go of what is no longer in it. */
  #take(library: Library): void {
    this.#library = library
    this.#failure = null
    if (!library.notebooks.some((n) => n.id === this.#notebook)) this.#notebook = library.notebooks[0]?.id ?? null
    if (this.#tag !== null && !this.tags.some((t) => t.tag === this.#tag)) this.#tag = null
    // the note that was open is gone (deleted here, or by an agent)
    const open = this.#note
    if (open && !library.notes.some((n) => n.id === open.id)) this.#close()
  }

  /** Show a note the backend sent: what it says is what is being written from now on. */
  #adopt(note: Note): void {
    this.#note = note
    this.#notebook = note.notebook
    this.#saved = note.body
    this.body = note.body
    this.#titleProblem = null
    this.#pane = 'note'
    this.#browsing = false
    // there is nothing to read in an empty note
    if (note.body === '') this.#mode = 'edit'
  }

  #close(): void {
    clearTimeout(this.#timer)
    this.#note = null
    this.#saved = ''
    this.body = ''
    this.#titleProblem = null
    this.#browsing = true
  }

  /** Run a call that answers with the library. It never rejects: a failure is kept, to be shown. */
  async #show(call: Promise<Library>): Promise<boolean> {
    try {
      this.#take(await call)
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** Run a call that answers with a note and the library. A refusal is the caller's to show, where it belongs. */
  async #open(call: Promise<Opened>, refused?: (reason: string) => void): Promise<boolean> {
    try {
      const opened = await call
      this.#take(opened.library)
      this.#adopt(opened.note)
      return true
    } catch (e) {
      const reason = refusalOf(e)
      if (reason !== null && refused) refused(reason)
      else this.#failure = messageOf(e)
      return false
    }
  }

  /** Read the library. */
  async load(): Promise<void> {
    await this.#show(this.#rpc.library())
  }

  /**
   * Hear the backend while the window is open: an agent wrote a note, or the host connection came back (a change made
   * while it was away reached nobody). The open note is read again unless it is being written: what the person typed
   * is not replaced under them. Returns a function that stops listening.
   */
  listen(): () => void {
    const again = async (): Promise<void> => {
      await this.load()
      const open = this.#note
      if (!open || this.status !== 'saved') return
      try {
        const note = await this.#rpc.read({ note: open.id })
        if (this.#note?.id === note.id && this.status === 'saved') {
          this.#note = note
          this.#saved = note.body
          this.body = note.body
        }
      } catch (e) {
        if (refusalOf(e) === null) this.#failure = messageOf(e)
      }
    }
    const stops = [this.#events.on('notebooks.changed', () => void again()), this.#events.on('host:reconnected', () => void again())]
    return () => {
      stops.forEach((stop) => stop())
      clearTimeout(this.#timer)
    }
  }

  /** The person typed in the open note. It is saved a moment after they stop. */
  edit(text: string): void {
    this.body = text
    clearTimeout(this.#timer)
    if (this.#autosave !== null) this.#timer = setTimeout(() => void this.flush(), this.#autosave)
  }

  /**
   * Send what was written, when the backend does not have it yet. Answers whether the backend now has it: a note is
   * not left while it does not, so nothing typed is lost to a failed save.
   */
  async flush(): Promise<boolean> {
    clearTimeout(this.#timer)
    const note = this.#note
    if (!note || this.body === this.#saved) return true
    const sent = this.body
    this.#saving = true
    try {
      const library = await this.#rpc.save({ note: note.id, body: sent })
      if (this.#note?.id === note.id) this.#saved = sent
      this.#take(library)
      return this.body === this.#saved
    } catch (e) {
      this.#failure = messageOf(e)
      // refused: the note is gone, and there is nothing to keep the text for
      if (refusalOf(e) !== null) {
        this.#close()
        void this.load()
      }
      return false
    } finally {
      this.#saving = false
    }
  }

  /** Show a notebook: its notes, with none open. */
  async selectNotebook(id: number): Promise<void> {
    if (id === this.#notebook || !(await this.flush())) return
    this.#close()
    this.#notebook = id
    this.#tag = null
    this.#query = ''
    this.#hits = null
  }

  /** Open a note. */
  async open(id: number): Promise<void> {
    this.switching = false
    if (this.#note?.id === id) {
      this.#pane = 'note'
      this.#browsing = false
      return
    }
    if (!(await this.flush())) return
    try {
      this.#adopt(await this.#rpc.read({ note: id }))
      this.#failure = null
    } catch (e) {
      this.#failure = messageOf(e)
      if (refusalOf(e) !== null) void this.load()
    }
  }

  /** Make a note called Untitled in the notebook that is shown, and open it to be written. */
  async create(): Promise<void> {
    const notebook = this.#notebook
    if (notebook === null || !(await this.flush())) return
    await this.#open(this.#rpc.create({ notebook }))
  }

  /** Follow a link: open the note of the notebook with that title, made when there is none. */
  async follow(title: string): Promise<void> {
    this.switching = false
    const notebook = this.#notebook
    if (notebook === null || title.trim() === '' || !(await this.flush())) return
    await this.#open(this.#rpc.follow({ notebook, title: title.trim() }))
  }

  /** Open today's note: the one named after the day, made the first time it is asked for. */
  daily(): Promise<void> {
    return this.follow(dayOf(this.#now()))
  }

  /** What is wrong with `title` as the open note's: the rules of a title, and the titles the notebook already has. */
  #judge(title: string): string | null {
    const problem = titleProblem(title)
    if (problem) return problem
    const other = named(this.#library.notes, this.#notebook, title)
    return other && other.id !== this.#note?.id ? `There is already a note called ${title.trim()}.` : null
  }

  /** The person is typing a title. A title that was wrong is judged again at each keystroke, until it is right. */
  retitle(title: string): void {
    if (this.#titleProblem !== null) this.#titleProblem = this.#judge(title)
  }

  /**
   * Give the open note another title: the links to it follow. A title it cannot have is not sent: what is wrong with it
   * is said at the field (`titleProblem`), as is the backend's refusal.
   */
  async rename(title: string): Promise<void> {
    const note = this.#note
    if (!note) return
    this.#titleProblem = this.#judge(title)
    if (this.#titleProblem !== null || title.trim() === note.title || !(await this.flush())) return
    await this.#open(this.#rpc.rename({ note: note.id, title: title.trim() }), (reason) => (this.#titleProblem = reason))
  }

  /** Bookmark the open note, or take its bookmark away. */
  async pin(): Promise<void> {
    const note = this.#note
    if (!note) return
    if (await this.#show(this.#rpc.pin({ note: note.id, pinned: !note.pinned }))) {
      if (this.#note?.id === note.id) this.#note = { ...this.#note, pinned: !note.pinned }
    }
  }

  /** Delete the open note. The window goes on to the list. */
  async remove(): Promise<void> {
    const note = this.#note
    if (!note) return
    clearTimeout(this.#timer)
    await this.#show(this.#rpc.remove({ note: note.id }))
  }

  /** Tick the task on a line of the open note, or untick it, and save. */
  async tick(line: number): Promise<void> {
    this.body = toggleTask(this.body, line)
    await this.flush()
  }

  setMode(mode: Mode): void {
    this.#mode = mode
  }

  /** Show the graph of the notebook, or go back to the note. */
  show(pane: Pane): void {
    this.#pane = pane
    this.#browsing = false
  }

  /** Leave the note for the list (a narrow window shows one or the other). */
  back(): void {
    this.#browsing = true
  }

  /** Narrow the list to the notes with a tag; the same tag again shows them all. */
  filter(tag: string): void {
    this.#tag = this.#tag === tag ? null : tag
    this.#browsing = true
  }

  /** Look for words in the notes of the notebook that is shown. Nothing typed shows the notes again. */
  async search(query: string): Promise<void> {
    this.#query = query
    const asked = ++this.#searches
    const notebook = this.#notebook
    if (query.trim() === '' || notebook === null) {
      this.#hits = null
      return
    }
    try {
      const found = await this.#rpc.search({ notebook, query })
      // an answer to words that were typed over is not shown
      if (asked === this.#searches) this.#hits = found.hits
      this.#failure = null
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** The person is typing a notebook's name: what was wrong with the last one no longer is. */
  nameNotebook(name: string): void {
    this.notebookDraft = name
    this.#notebookProblem = null
  }

  /**
   * Add a notebook called what is typed, and show it. A name another notebook has is not sent: what is wrong is said at
   * the field (`notebookProblem`), as is the backend's refusal. Answers whether it was added.
   */
  async addNotebook(): Promise<boolean> {
    const name = this.notebookDraft.trim()
    if (name === '') return false
    if (this.#library.notebooks.some((n) => keyOf(n.name) === keyOf(name))) {
      this.#notebookProblem = `There is already a notebook called ${name}.`
      return false
    }
    if (!(await this.flush())) return false
    const before = new Set(this.#library.notebooks.map((n) => n.id))
    try {
      this.#take(await this.#rpc.addNotebook({ name }))
    } catch (e) {
      const reason = refusalOf(e)
      if (reason !== null) this.#notebookProblem = reason
      else this.#failure = messageOf(e)
      return false
    }
    this.notebookDraft = ''
    const added = this.#library.notebooks.find((n) => !before.has(n.id))
    if (added) await this.selectNotebook(added.id)
    return true
  }

  /** Delete the notebook that is shown and the notes in it. The window goes on to the first notebook. */
  async removeNotebook(): Promise<void> {
    const notebook = this.#notebook
    if (notebook === null) return
    clearTimeout(this.#timer)
    // what was being written in it goes with it
    this.body = this.#saved
    if (await this.#show(this.#rpc.removeNotebook({ notebook }))) {
      this.#query = ''
      this.#hits = null
    }
  }
}
