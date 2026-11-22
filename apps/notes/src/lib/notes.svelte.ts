import type { Api, Board, Folder, Host, Note } from 'app:rpc'
import { folderProblem } from './folders'
import { ALL, DELETED, UNTITLED, folderView, sameView, sections, shown, type Section, type View } from './views'

/** What the window needs of the backend: the board, the text of a note, and each way of changing them. */
export type NotesRpc = Pick<
  Api,
  'board' | 'addFolder' | 'removeFolder' | 'create' | 'read' | 'write' | 'pin' | 'move' | 'remove' | 'restore' | 'purge' | 'search'
>
/** Where the backend's events are heard (`host.on`). */
export type NotesEvents = Pick<Host, 'on'>

/** What is in front in a window too narrow for all three: the folders, the notes of one, or a note. */
export type Pane = 'folders' | 'list' | 'note'

/** The code of a call the backend refused (`ctx:fail`): it is about what was asked, not about the host. */
const REFUSED = -32010
const isRefusal = (e: unknown): boolean => typeof e === 'object' && e !== null && (e as { code?: unknown }).code === REFUSED
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's Notes. The folders and the notes are the backend's: every change is a call that answers with the whole
 * board, and this holds the last one. What is the window's own is here: which folder is shown, which note is open and
 * what is typed in it but not saved yet, and what is being searched for.
 */
export class Notes {
  // `raw`: the board is replaced whole by each answer, never edited in place
  #board = $state.raw<Board>({ folders: [], notes: [] })
  // why the last call failed: the backend's own message, or the host's. It stays until a call works
  #failure = $state<string | null>(null)
  #view = $state.raw<View>(ALL)
  #pane = $state<Pane>('list')
  #now = $state.raw(new Date())

  // the id of the note that is open, and whether its text has been read
  #open = $state<number | null>(null)
  #loaded = $state(false)
  // the text the backend has for the open note, as far as this window knows: `text` differs while there is something to save
  #saved = $state('')
  /** What the open note says in this window: what is typed, saved or not. */
  text = $state('')

  #query = $state('')
  // the ids of the notes the search found; none while nothing is searched for
  #found = $state.raw<ReadonlySet<number> | null>(null)

  #folderDraft = $state('')
  // why the backend refused the folder that was typed; gone when the name changes
  #folderRefusal = $state<string | null>(null)

  readonly #rpc: NotesRpc
  readonly #events: NotesEvents
  readonly #clock: () => Date

  /** `now` is the person's clock: it decides which notes are today's. */
  constructor(rpc: NotesRpc, events: NotesEvents, now: () => Date = () => new Date()) {
    this.#rpc = rpc
    this.#events = events
    this.#clock = now
    this.#now = now()
  }

  get folders(): readonly Folder[] {
    return this.#board.folders
  }
  get view(): View {
    return this.#view
  }
  get pane(): Pane {
    return this.#pane
  }
  /** The moment the board was last read: what "today" is measured from. */
  get now(): Date {
    return this.#now
  }
  get failure(): string | null {
    return this.#failure
  }
  /** What is searched for. */
  get query(): string {
    return this.#query
  }
  /** Whether the open note's text is here to be edited. */
  get loaded(): boolean {
    return this.#loaded
  }
  /** What is being typed as the name of the next folder. */
  get folderDraft(): string {
    return this.#folderDraft
  }
  set folderDraft(name: string) {
    this.#folderDraft = name
    this.#folderRefusal = null
  }

  /** Whether notes are being searched: the list then shows what was found, from every folder. */
  readonly searching: boolean = $derived(this.#query.trim() !== '')
  /** The folder that is shown, when one is. */
  readonly folder: Folder | undefined = $derived.by(() => {
    const view = this.#view
    return view.kind === 'folder' ? this.#board.folders.find((f) => f.id === view.id) : undefined
  })
  /** The name of what is shown. */
  readonly title: string = $derived(
    this.#view.kind === 'all' ? 'All Notes' : this.#view.kind === 'deleted' ? 'Recently Deleted' : (this.folder?.name ?? '')
  )
  /** The notes in the list: those of the view, or those the search found, the most recently edited first. */
  readonly notes: Note[] = $derived.by(() => {
    const found = this.#found
    if (this.searching) return found ? this.#board.notes.filter((n) => !n.deleted && found.has(n.id)) : []
    return shown(this.#board.notes, this.#view)
  })
  /** The notes of the list under their headings. Deleted notes have no pins: they are by date alone. */
  readonly sections: Section[] = $derived(sections(this.notes, this.#now))
  /** The note that is open. */
  readonly note: Note | undefined = $derived(this.#board.notes.find((n) => n.id === this.#open))
  /** How many notes there are: in all, deleted, and in each folder by the folder's id. */
  readonly counts: { readonly all: number; readonly deleted: number; readonly folders: ReadonlyMap<number, number> } = $derived.by(() => {
    const folders = new Map<number, number>()
    let deleted = 0
    for (const note of this.#board.notes) {
      if (note.deleted) deleted++
      else folders.set(note.folder, (folders.get(note.folder) ?? 0) + 1)
    }
    return { all: this.#board.notes.length - deleted, deleted, folders }
  })
  /** Whether a note can be started here: not among the deleted ones, and there is a folder to put it in. */
  readonly creatable: boolean = $derived(this.#view.kind !== 'deleted' && this.#board.folders.length > 0)
  /** Whether the folder that is shown can be deleted: the first one stays. */
  readonly removable: boolean = $derived(this.folder !== undefined && this.folder.id !== this.#board.folders[0]?.id)
  /** What is wrong with the folder name that is typed: it is taken, or the backend refused it. */
  readonly folderProblem: string | null = $derived(folderProblem(this.#folderDraft, this.#board.folders) ?? this.#folderRefusal)
  /** Whether there is something typed that the backend does not have. */
  readonly unsaved: boolean = $derived(this.#open !== null && this.#loaded && this.text !== this.#saved)

  /** What a note is called: its first line, or `New Note` while it has none. */
  titleOf(note: Note): string {
    return note.title === '' ? UNTITLED : note.title
  }

  /** Run a call that answers with the board. It never rejects: a failure is kept, to be shown. */
  async #show(call: Promise<Board>): Promise<boolean> {
    try {
      const board = await call
      this.#board = board
      this.#now = this.#clock()
      this.#failure = null
      // the folder that was shown is gone (deleted here, or in another window)
      const view = this.#view
      if (view.kind === 'folder' && !board.folders.some((f) => f.id === view.id)) this.#view = ALL
      if (this.#open !== null && !board.notes.some((n) => n.id === this.#open)) this.#close()
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** No note is open; a narrow window goes back to the list. */
  #close(): void {
    this.#open = null
    this.#loaded = false
    this.text = ''
    this.#saved = ''
    if (this.#pane === 'note') this.#pane = 'list'
  }

  /** Read the open note's text, unless something is typed that it would write over. */
  async #read(): Promise<void> {
    const id = this.#open
    if (id === null || this.unsaved) return
    try {
      const { body } = await this.#rpc.read({ note: id })
      if (this.#open !== id || this.unsaved) return
      this.text = body
      this.#saved = body
      this.#loaded = true
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Read the board, and with it the open note and what is searched for: any of them may have changed elsewhere. */
  async load(): Promise<void> {
    if (!(await this.#show(this.#rpc.board()))) return
    await Promise.all([this.#read(), this.searching ? this.search(this.#query) : undefined])
  }

  /**
   * Hear the backend while the window is open: an agent changed the notes, or the host connection came back (a change
   * made while it was away reached nobody). Returns a function that stops listening.
   */
  listen(): () => void {
    const stops = [
      this.#events.on('notes.changed', () => void this.load()),
      this.#events.on('host:reconnected', () => void this.load())
    ]
    return () => stops.forEach((stop) => stop())
  }

  /**
   * Leave the open note: what is typed is saved first, and a note with nothing in it is not kept (the backend deletes a
   * blank note for good).
   */
  async #leave(): Promise<void> {
    const note = this.note
    if (!note || !this.#loaded) return
    if (note.deleted) return
    if (this.text.trim() === '') await this.#show(this.#rpc.remove({ note: note.id }))
    else await this.save()
  }

  /** Save what is typed in the open note. It is typed over nothing: the text stays as it is if the call fails, to be saved again. */
  async save(): Promise<void> {
    const note = this.note
    if (!note || note.deleted || !this.unsaved) return
    const before = this.#saved
    const body = this.text
    this.#saved = body
    if (!(await this.#show(this.#rpc.write({ note: note.id, body }))) && this.#open === note.id && this.#saved === body) this.#saved = before
  }

  /** Show All Notes, a folder or Recently Deleted. The search is over. */
  async select(view: View): Promise<void> {
    await this.#leave()
    this.#close()
    this.#view = view
    this.#pane = 'list'
    this.#query = ''
    this.#found = null
  }

  /** Step back in a narrow window: from a note to the list, from the list to the folders. */
  async back(): Promise<void> {
    if (this.#pane === 'note') {
      await this.#leave()
      this.#close()
      this.#pane = 'list'
    } else this.#pane = 'folders'
  }

  /** Open a note: its text is read, and it is the one in front. */
  async open(note: Note): Promise<void> {
    if (this.#open === note.id) {
      this.#pane = 'note'
      return
    }
    await this.#leave()
    this.#open = note.id
    this.#loaded = false
    this.text = ''
    this.#saved = ''
    this.#pane = 'note'
    await this.#read()
  }

  /** Start a note and open it: in the folder that is shown, or in the first folder from All Notes. */
  async create(): Promise<void> {
    if (!this.creatable) return
    await this.#leave()
    const view = this.#view
    const folder = view.kind === 'folder' ? view.id : this.#board.folders[0]?.id
    if (folder === undefined) return
    const before = new Set(this.#board.notes.map((n) => n.id))
    if (!(await this.#show(this.#rpc.create({ folder })))) return
    const made = this.#board.notes.find((n) => !before.has(n.id))
    if (!made) return
    this.#query = ''
    this.#found = null
    this.#open = made.id
    this.text = ''
    this.#saved = ''
    this.#loaded = true
    this.#pane = 'note'
  }

  /** Search every note for what is typed. Only the answer to what is typed now is kept. */
  async search(query: string): Promise<void> {
    this.#query = query
    const asked = query.trim()
    if (asked === '') {
      this.#found = null
      return
    }
    try {
      const { notes } = await this.#rpc.search({ query: asked })
      if (this.#query === query) this.#found = new Set(notes)
    } catch (e) {
      this.#failure = messageOf(e)
    }
  }

  /** Pin a note to the top of its list, or unpin it. */
  pin(note: Note): Promise<boolean> {
    return this.#show(this.#rpc.pin({ note: note.id, pinned: !note.pinned }))
  }

  /** Move a note to another folder. In the folder it leaves, it is no longer in the list, and stays open. */
  async move(note: Note, folder: number): Promise<void> {
    if (folder !== note.folder) await this.#show(this.#rpc.move({ note: note.id, folder }))
  }

  /** Delete a note: it goes to Recently Deleted with what was typed in it. */
  async remove(note: Note): Promise<void> {
    if (note.id === this.#open) await this.save()
    if (note.id === this.#open) this.#close()
    await this.#show(this.#rpc.remove({ note: note.id }))
  }

  /** Bring a deleted note back to its folder. It leaves Recently Deleted, so it is no longer open. */
  async restore(note: Note): Promise<void> {
    if (note.id === this.#open) this.#close()
    await this.#show(this.#rpc.restore({ note: note.id }))
  }

  /** Delete a deleted note for good. */
  async purge(note: Note): Promise<void> {
    if (note.id === this.#open) this.#close()
    await this.#show(this.#rpc.purge({ note: note.id }))
  }

  /**
   * Add a folder called what is typed, and show it. A name that is taken is not sent. A refusal is about the name, and
   * is kept beside it; any other failure is the window's. The name comes back when the folder could not be added.
   */
  async addFolder(): Promise<void> {
    const name = this.#folderDraft.trim()
    if (name === '' || this.folderProblem !== null) return
    const typed = this.#folderDraft
    const before = new Set(this.#board.folders.map((f) => f.id))
    this.folderDraft = ''
    try {
      this.#board = await this.#rpc.addFolder({ name })
      this.#failure = null
    } catch (e) {
      this.#folderDraft = typed
      if (isRefusal(e)) this.#folderRefusal = messageOf(e)
      else this.#failure = messageOf(e)
      return
    }
    const added = this.#board.folders.find((f) => !before.has(f.id))
    if (added) await this.select(folderView(added.id))
  }

  /** Delete a folder: its notes go to Recently Deleted. The window goes on to All Notes. */
  async removeFolder(folder: Folder): Promise<void> {
    const open = this.note
    if (open && open.folder === folder.id) await this.#leave()
    await this.#show(this.#rpc.removeFolder({ folder: folder.id }))
    if (sameView(this.#view, DELETED)) return
    // the note that was open went with its folder: it is among the deleted ones now
    if (this.note?.deleted) this.#close()
  }
}
