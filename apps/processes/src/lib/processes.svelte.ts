import type { Api, Detail, Host, Overview, ProcessRef, ProcessRow, Snapshot } from 'app:rpc'

/** What the window needs of the backend: a reading, choosing a process, letting it go, and ending it. */
export type ProcessesRpc = Pick<Api, 'snapshot' | 'select' | 'release' | 'terminate'>
/** Where the backend's events are heard (`host.on`). */
export type ProcessesEvents = Pick<Host, 'on'>

/** The two things the window shows: the machine as a whole, and its programs. */
export type View = 'overview' | 'processes'
/** What the processes are in the order of. */
export type Sort = 'cpu' | 'memory' | 'pid' | 'name'
/** How often the machine is read again, in seconds; 0 is never. */
export type Interval = 0 | 1 | 2 | 5
/** How a program is ended: asked to quit, or stopped at once. */
export type Ending = 'terminate' | 'force'
/** The column a narrow window shows (`ui-split-view`'s `column`). */
export type Pane = 'sidebar' | 'content' | 'detail'

/** Why a program was not ended: the backend looked at what was asked and said no (its sentence, and the program's name). */
export interface Refusal {
  name: string
  message: string
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
/** The backend looked at what was asked and said no (`ctx:fail`): the message is about that, not about the call. */
const isRefusal = (e: unknown): boolean => (e as { code?: unknown } | null)?.code === -32010

/** What the window says when the backend no longer holds the choice (it was let go, or the backend started afresh). */
export const LOST = 'Processes no longer holds this program. Choose it again to end it.'

const NO_ROWS: readonly ProcessRow[] = []

/**
 * Whether two references name the same process: the PID alone does not (a PID is reused), nor does the PID with when it
 * started (the same second), so the backend's number for which sighting of the PID it is counts too.
 */
export const same = (a: ProcessRef | null | undefined, b: ProcessRef | null | undefined): boolean =>
  !!a && !!b && a.pid === b.pid && a.startTime === b.startTime && a.observation === b.observation

const refOf = ({ pid, startTime, observation }: ProcessRef): ProcessRef => ({ pid, startTime, observation })

/** A value that may be missing sorts after every one that is there. */
const byDescending = (a: number | undefined, b: number | undefined): number =>
  a === undefined && b === undefined ? 0 : a === undefined ? 1 : b === undefined ? -1 : b - a

const COMPARE: Readonly<Record<Sort, (a: ProcessRow, b: ProcessRow) => number>> = {
  cpu: (a, b) => byDescending(a.cpu, b.cpu),
  memory: (a, b) => b.memory - a.memory,
  pid: (a, b) => a.pid - b.pid,
  name: (a, b) => a.name.localeCompare(b.name, 'en', { sensitivity: 'base' })
}

/** The rows that match what was typed: a name that has it in it, or a PID that starts with it. */
export function matching(rows: readonly ProcessRow[], query: string): readonly ProcessRow[] {
  const text = query.trim().toLowerCase()
  if (text === '') return rows
  return rows.filter((row) => row.name.toLowerCase().includes(text) || String(row.pid).startsWith(text))
}

/** The rows in an order; a tie goes to the lower PID, so that rows do not swap places from one reading to the next. */
export const sorted = (rows: readonly ProcessRow[], sort: Sort): readonly ProcessRow[] =>
  [...rows].sort((a, b) => COMPARE[sort](a, b) || a.pid - b.pid)

/**
 * One window's Processes. The machine is the backend's: every reading is a call that answers with the whole of it,
 * and this holds the last one. What is the window's own is here: what it shows (the view, the search, the order), how
 * often it reads, the process the person chose, and the question before one is ended.
 */
export class Processes {
  // `raw`: a reading is replaced whole by each answer, never edited in place
  #snapshot = $state.raw<Snapshot | null>(null)
  // why the last call failed (the host's message, or the backend's): not why a program was not ended
  #failure = $state<string | null>(null)
  #view = $state<View>('processes')
  #pane = $state<Pane>('content')
  #query = $state('')
  #sort = $state<Sort>('cpu')
  #interval = $state<Interval>(2)
  // the process the person chose, and everything the backend said of it in the last reading
  #selected = $state.raw<ProcessRef | null>(null)
  // the backend's id for this window's choice: the only way to act on the process, and none other is made up here
  #selection = $state<string | null>(null)
  // the backend said it no longer holds that id: the process is still shown, as it was, and is chosen again to be ended
  #lostHold = $state(false)
  #detail = $state.raw<Detail | null>(null)
  // the process a call to choose is in flight for
  #choosing = $state.raw<ProcessRef | null>(null)
  // how the person has been asked to end the chosen process
  #asking = $state<Ending | null>(null)
  #ending = $state(false)
  #refused = $state.raw<Refusal | null>(null)
  #notice = $state<string | null>(null)
  #updatedAt = $state<number | null>(null)
  #reading = false

  readonly #rpc: ProcessesRpc
  readonly #events: ProcessesEvents
  readonly #now: () => number

  constructor(rpc: ProcessesRpc, events: ProcessesEvents, now: () => number = Date.now) {
    this.#rpc = rpc
    this.#events = events
    this.#now = now
  }

  /** The first reading has arrived. */
  get loaded(): boolean {
    return this.#snapshot !== null
  }
  get overview(): Overview | null {
    return this.#snapshot?.overview ?? null
  }
  /** Every process of the last reading, in the order of their PIDs. */
  get processes(): readonly ProcessRow[] {
    return this.#snapshot?.processes ?? NO_ROWS
  }
  get failure(): string | null {
    return this.#failure
  }
  get view(): View {
    return this.#view
  }
  /** The column a narrow window shows. */
  get pane(): Pane {
    return this.#pane
  }
  get query(): string {
    return this.#query
  }
  get sort(): Sort {
    return this.#sort
  }
  get interval(): Interval {
    return this.#interval
  }
  /** The process the person chose, as the last reading shows it. */
  get detail(): Detail | null {
    return this.#detail
  }
  get selected(): ProcessRef | null {
    return this.#selected
  }
  /** A process is being chosen: the list marks it already. */
  get choosing(): ProcessRef | null {
    return this.#choosing
  }
  /** The question the person is asked before the chosen process is ended. */
  get asking(): Ending | null {
    return this.#asking
  }
  /** The chosen process is being ended. */
  get ending(): boolean {
    return this.#ending
  }
  /** Why the chosen process was not ended. */
  get refused(): Refusal | null {
    return this.#refused
  }
  /** What was done to the chosen process: said until the person chooses another. */
  get notice(): string | null {
    return this.#notice
  }
  /** When the machine was last read, in milliseconds since the epoch. */
  get updatedAt(): number | null {
    return this.#updatedAt
  }

  /** The processes that match the search, in the order chosen. */
  readonly rows: readonly ProcessRow[] = $derived(sorted(matching(this.processes, this.#query), this.#sort))

  /** The chosen process can be ended: it is still there, the backend holds it for ending, and no call to end it is on its way. */
  readonly endable: boolean = $derived(this.#detail?.alive === true && this.#detail.control.available && !this.#ending)

  /** Why the chosen process, which is there, cannot be ended: it can still be looked at. */
  readonly unavailable: string | null = $derived(
    this.#detail?.alive === true && !this.#detail.control.available ? (this.#detail.control.reason ?? null) : null
  )

  /**
   * Read the machine. Never rejects: a failure is kept, to be shown. Readings do not overlap: one asked for while
   * another is on its way is the one on its way (a window that reads every second must not queue behind a slow host).
   */
  async refresh(): Promise<void> {
    if (this.#reading) return
    this.#reading = true
    const asked = this.#selected
    const selection = this.#selection
    try {
      const snapshot = await this.#rpc.snapshot(selection ? { selected: selection } : undefined)
      this.#snapshot = snapshot
      this.#updatedAt = this.#now()
      this.#failure = null
      // the person may have chosen another process, or none, while the backend measured: that answer is not for them
      if (asked && selection && this.#selection === selection) {
        this.#lostHold = snapshot.selectionLost === true
        this.#detail = snapshot.detail ?? (snapshot.selectionLost ? this.#lost() : this.#gone())
      }
    } catch (e) {
      this.#failure = messageOf(e)
    } finally {
      this.#reading = false
    }
  }

  /** The chosen process as it was last seen, now that the backend does not know it any more. */
  #gone(): Detail | null {
    const last = this.#detail
    return last
      ? { ...last, alive: false, parent: undefined, children: [], control: { available: false, reason: `${last.name} has quit.` } }
      : null
  }

  /** The chosen process as it was last seen, now that the backend does not hold the choice: it cannot be ended. */
  #lost(): Detail | null {
    const last = this.#detail
    return last ? { ...last, control: { available: false, reason: LOST } } : null
  }

  /** Read for the first time. */
  async load(): Promise<void> {
    await this.refresh()
  }

  /**
   * Hear the host while the window is open: its connection came back (the backend may have changed while it was away).
   * Returns a function that stops listening.
   */
  listen(): () => void {
    return this.#events.on('host:reconnected', () => void this.refresh())
  }

  /** Show the machine as a whole, or its programs. */
  show(view: View): void {
    this.#view = view
    this.#pane = view === 'overview' ? 'detail' : 'content'
  }

  /** The way back one column in a narrow window: from a process to the list, from a list to the views. */
  back(): void {
    if (this.#pane === 'detail' && this.#view === 'processes') {
      this.close()
      this.#pane = 'content'
    } else {
      this.#pane = 'sidebar'
    }
  }

  search(text: string): void {
    this.#query = text
  }

  order(sort: Sort): void {
    this.#sort = sort
  }

  /** How often to read again; 0 stops. */
  every(interval: Interval): void {
    this.#interval = interval
  }

  /**
   * Choose a process to look at and to end. The backend holds it by a reference that a reused PID cannot move; the one
   * chosen before is let go. A process that has quit since the list was read is said so, and the list is read again.
   */
  async choose(row: ProcessRef): Promise<boolean> {
    if (same(this.#selected, row) && this.#detail && !this.#lostHold) {
      this.#pane = 'detail'
      return true
    }
    const ref = refOf(row)
    this.#choosing = ref
    this.#pane = 'detail'
    try {
      const { selection, detail } = await this.#rpc.select({ process: ref })
      if (!same(this.#choosing, ref)) {
        // another was chosen while this was asked for: this one is not wanted
        void this.#rpc.release({ selection }).catch(() => {})
        return false
      }
      this.#letGo()
      this.#selected = ref
      this.#selection = selection
      this.#lostHold = false
      this.#detail = detail
      this.#asking = null
      this.#refused = null
      this.#notice = null
      this.#failure = null
      return true
    } catch (e) {
      if (same(this.#choosing, ref)) {
        this.#failure = messageOf(e)
        // a program that was gone: the list shows it no more
        if (isRefusal(e)) void this.refresh()
        if (!this.#selected) this.#pane = 'content'
      }
      return false
    } finally {
      if (same(this.#choosing, ref)) this.#choosing = null
    }
  }

  /** Let the chosen process go: the backend lets go of its reference. */
  #letGo(): void {
    const held = this.#selection
    if (held) void this.#rpc.release({ selection: held }).catch(() => {})
    this.#selected = null
    this.#selection = null
    this.#lostHold = false
    this.#detail = null
  }

  /** Stop looking at the chosen process. */
  close(): void {
    this.#choosing = null
    this.#letGo()
    this.#asking = null
    this.#refused = null
    this.#notice = null
  }

  /** Ask before the chosen process is ended: once it is stopped there is no taking it back. */
  ask(ending: Ending): void {
    if (this.#detail?.alive && this.#detail.control.available) {
      this.#asking = ending
      this.#refused = null
    }
  }

  /** The answer was Cancel. */
  keep(): void {
    this.#asking = null
  }

  /**
   * The answer was to go ahead: end the chosen process the way that was asked about, through the id the backend
   * issued for this window's choice. The process is never chosen again on the way: a backend that no longer holds the
   * choice (it let it go, or started afresh) refuses, and the person chooses the program again. What the system
   * refused is said at the process; the rest is the window's.
   */
  async confirm(): Promise<boolean> {
    const ending = this.#asking
    const process = this.#selected
    const selection = this.#selection
    const detail = this.#detail
    this.#asking = null
    if (ending === null || process === null || selection === null || detail === null) return false
    const name = detail.name
    this.#ending = true
    this.#refused = null
    this.#notice = null
    try {
      await this.#rpc.terminate({ selection, force: ending === 'force' })
      if (same(this.#selected, process)) this.#notice = ending === 'force' ? `Stopped ${name}.` : `Asked ${name} to quit.`
      this.#failure = null
      return true
    } catch (e) {
      if (isRefusal(e)) this.#refused = { name, message: messageOf(e) }
      else this.#failure = messageOf(e)
      return false
    } finally {
      this.#ending = false
      // what the program did with it, as soon as it can be told
      void this.refresh()
    }
  }
}
