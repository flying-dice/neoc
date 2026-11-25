import type { Api, Board, List, Todo } from '@todos/rpc'
import type { Host } from '@vos/host'
import { dayOf } from './dates'
import { COLORS, SMART, listView, sameView, shown, smart, type SmartId, type View } from './views'

/** What the window needs of the backend: the board, and each way of changing it. */
export type TodosRpc = Pick<Api, 'board' | 'addList' | 'removeList' | 'add' | 'update' | 'toggle' | 'remove'>
/** Where the backend's events are heard (`host.on`). */
export type TodosEvents = Pick<Host, 'on'>

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/**
 * One window's Todos. The lists and the todos are the backend's: every change is a call that answers with the whole
 * board, and this holds the last one. What is the window's own is here: which list is shown, which todo is open for
 * its details, and what is being typed.
 */
export class Todos {
  // `raw`: the board is replaced whole by each answer, never edited in place
  #board = $state.raw<Board>({ lists: [], todos: [] })
  // why the last call failed: the backend's own message, or the host's. It stays until a call works
  #failure = $state<string | null>(null)
  #view = $state.raw<View>(smart('today'))
  // a window too narrow for both shows the lists until one is picked
  #browsing = $state(true)
  #expanded = $state<number | null>(null)
  #today = $state('')
  /** What is being typed as the next todo. */
  draft = $state('')
  /** What is being typed as the name of the next list. */
  listDraft = $state('')

  readonly #rpc: TodosRpc
  readonly #events: TodosEvents
  readonly #now: () => Date

  /** `now` is the person's clock: it decides which day is today. */
  constructor(rpc: TodosRpc, events: TodosEvents, now: () => Date = () => new Date()) {
    this.#rpc = rpc
    this.#events = events
    this.#now = now
    this.#today = dayOf(now())
  }

  get lists(): readonly List[] {
    return this.#board.lists
  }
  get view(): View {
    return this.#view
  }
  /** Today, as `2026-06-09`: read from the clock each time the board is. */
  get today(): string {
    return this.#today
  }
  get failure(): string | null {
    return this.#failure
  }
  /** The lists are in front, not the todos of one (it matters only in a window too narrow for both). */
  get browsing(): boolean {
    return this.#browsing
  }
  /** The id of the todo whose details are open. */
  get expanded(): number | null {
    return this.#expanded
  }

  /** The list that is shown, when it is one of the person's own. */
  readonly list: List | undefined = $derived(
    this.#view.kind === 'list' ? this.#board.lists.find((l) => l.id === this.#view.id) : undefined
  )
  /** The name of what is shown. */
  readonly title: string = $derived.by(() => {
    const view = this.#view
    return view.kind === 'smart' ? (SMART.find((s) => s.id === view.id)?.name ?? '') : (this.list?.name ?? '')
  })
  /** The todos of the view that is shown, in its order. */
  readonly todos: Todo[] = $derived(shown(this.#board.todos, this.#view, this.#today))
  /** How many todos each smart list holds. */
  readonly counts: Readonly<Record<SmartId, number>> = $derived.by(() => {
    const count = (id: SmartId): number => shown(this.#board.todos, smart(id), this.#today).length
    return { today: count('today'), scheduled: count('scheduled'), all: count('all'), flagged: count('flagged'), completed: count('completed') }
  })
  /** How many todos are still to do in each list, by the list's id. */
  readonly remaining: ReadonlyMap<number, number> = $derived.by(() => {
    const left = new Map<number, number>()
    for (const todo of this.#board.todos) if (!todo.done) left.set(todo.list, (left.get(todo.list) ?? 0) + 1)
    return left
  })
  /** Whether a todo can be added here: not among the completed ones, and there is a list to put it in. */
  readonly addable: boolean = $derived(!sameView(this.#view, smart('completed')) && this.#board.lists.length > 0)

  /** The name of the list a todo is in. */
  listName(todo: Todo): string {
    return this.#board.lists.find((l) => l.id === todo.list)?.name ?? ''
  }

  /** Run a call that answers with the board. It never rejects: a failure is kept, to be shown. */
  async #show(call: Promise<Board>): Promise<boolean> {
    try {
      const board = await call
      this.#board = board
      this.#today = dayOf(this.#now())
      this.#failure = null
      // the list that was shown is gone (deleted here, or in another window)
      const view = this.#view
      if (view.kind === 'list' && !board.lists.some((l) => l.id === view.id)) this.#view = smart('all')
      if (this.#expanded !== null && !board.todos.some((t) => t.id === this.#expanded)) this.#expanded = null
      return true
    } catch (e) {
      this.#failure = messageOf(e)
      return false
    }
  }

  /** Read the board. */
  async load(): Promise<void> {
    await this.#show(this.#rpc.board())
  }

  /**
   * Hear the backend while the window is open: an agent changed the todos, or the host connection came back (a change
   * made while it was away reached nobody). Returns a function that stops listening.
   */
  listen(): () => void {
    const stops = [
      this.#events.on('todos.changed', () => void this.load()),
      this.#events.on('host:reconnected', () => void this.load())
    ]
    return () => stops.forEach((stop) => stop())
  }

  /** Show a list, smart or the person's own. */
  select(view: View): void {
    this.#view = view
    this.#browsing = false
    this.#expanded = null
  }

  /** Leave the todos for the lists (a narrow window shows one or the other). */
  back(): void {
    this.#browsing = true
  }

  /** Open a todo's details, or close them when they are open. */
  expand(todo: Todo): void {
    this.#expanded = this.#expanded === todo.id ? null : todo.id
  }

  /**
   * Add what is typed as a todo. It goes in the list that is shown, or in the first list from a smart one, and it
   * takes what that smart list is about: due today from Today and Scheduled, flagged from Flagged. What was typed
   * comes back when it could not be added.
   */
  async add(): Promise<void> {
    const title = this.draft.trim()
    const view = this.#view
    const list = view.kind === 'list' ? view.id : this.#board.lists[0]?.id
    if (title === '' || list === undefined || !this.addable) return
    const typed = this.draft
    this.draft = ''
    const scheduled = view.kind === 'smart' && (view.id === 'today' || view.id === 'scheduled')
    const added = await this.#show(
      this.#rpc.add({
        list,
        title,
        ...(scheduled ? { due: dayOf(this.#now()) } : {}),
        ...(sameView(view, smart('flagged')) ? { flagged: true } : {})
      })
    )
    if (!added) this.draft = typed
  }

  /** Tick a todo off, or bring a completed one back. */
  toggle(todo: Todo): Promise<boolean> {
    return this.#show(this.#rpc.toggle({ todo: todo.id }))
  }

  flag(todo: Todo): Promise<boolean> {
    return this.#show(this.#rpc.update({ todo: todo.id, flagged: !todo.flagged }))
  }

  /** Give a todo another title. A blank one, or the one it has, changes nothing. */
  async rename(todo: Todo, title: string): Promise<void> {
    const next = title.trim()
    if (next !== '' && next !== todo.title) await this.#show(this.#rpc.update({ todo: todo.id, title: next }))
  }

  async annotate(todo: Todo, notes: string): Promise<void> {
    if (notes !== todo.notes) await this.#show(this.#rpc.update({ todo: todo.id, notes }))
  }

  /** Give a todo a day (`2026-06-09`), or take its day away (empty). */
  async schedule(todo: Todo, due: string): Promise<void> {
    if (due !== todo.due) await this.#show(this.#rpc.update({ todo: todo.id, due }))
  }

  remove(todo: Todo): Promise<boolean> {
    return this.#show(this.#rpc.remove({ todo: todo.id }))
  }

  /** Add a list called what is typed, in the next colour, and show it. The name comes back when it could not be added. */
  async addList(): Promise<void> {
    const name = this.listDraft.trim()
    if (name === '') return
    const typed = this.listDraft
    const before = new Set(this.#board.lists.map((l) => l.id))
    const color = COLORS[before.size % COLORS.length] ?? 'blue'
    this.listDraft = ''
    if (!(await this.#show(this.#rpc.addList({ name, color })))) {
      this.listDraft = typed
      return
    }
    const added = this.#board.lists.find((l) => !before.has(l.id))
    if (added) this.select(listView(added.id))
  }

  /** Delete a list and the todos in it. The window goes on to All, and to the lists when it is narrow. */
  async removeList(list: List): Promise<void> {
    const shownNow = sameView(this.#view, listView(list.id))
    if ((await this.#show(this.#rpc.removeList({ list: list.id }))) && shownNow) this.#browsing = true
  }
}
