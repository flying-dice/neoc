// One Telemetry window's state: the traces and logs it shows, what is picked, what is searched for, and the probes it
// sends. What it needs from outside arrives through the constructor (the reader, the backend, a way to run work later),
// so a test or a story builds one with fakes.
import {
  cutOff,
  EXPORT_LIMIT,
  exportText,
  logExport,
  logKey,
  matchingLogs,
  matchingTraces,
  traceExport,
  waterfall,
  type Bar,
  type Found,
  type Level,
  type Log,
  type LogQuery,
  type Source,
  type Span,
  type TraceSummary
} from './telemetry'

/** What was recorded, read through the host page (`vos/telemetry/query`, offered to this app's frame only). */
export interface TelemetryReader {
  traces(): Promise<TraceSummary[]>
  query(q: LogQuery): Promise<Found>
}

/**
 * Where what is copied goes (the page's `navigator.clipboard`, which the app's frame is allowed to write to). It is given
 * the text as a promise, started in the click: a browser that is strict about when the clipboard may be written (WebKit)
 * takes a write begun in the click, even when what it holds is read from the host afterwards.
 */
export interface Clipboard {
  write(text: Promise<string>): Promise<void>
}

/** The backend's one method: a call to try tracing with (the app's private `_probe` on the bus: `App.svelte` sends it). */
export interface ProbeRpc {
  probe(params: VosApp.Telemetry.ProbeParams): Promise<VosApp.Telemetry.ProbeResult>
}

/**
 * What the window traces its own work with (`vos.tracing`): a probe is a span, and its call a child of it; a failure it
 * catches and turns into something to show is a warning (the platform records a bus call's failure, not these).
 */
export interface Tracer {
  span(name: string, fields?: { readonly [key: string]: unknown }): { in_scope<T>(body: () => T): T }
  warn(message: string, fields?: { readonly [key: string]: unknown }): void
}

/** No tracing at all (a test, a story): the work runs as it is. */
export const untraced: Tracer = { span: () => ({ in_scope: (body) => body() }), warn: () => {} }

export interface TelemetryOptions {
  /** Runs the page's failing render outside any handler, as a framework's render would (a timer, by default). */
  later?: (fn: () => void) => void
  /** Traces the probes (by default, nothing does). */
  tracer?: Tracer
  /** Where a copy goes (by default, nowhere: a copy is refused). */
  clipboard?: Clipboard
  /** Runs work after a wait, in milliseconds (a timer, by default). */
  after?: (fn: () => void, ms: number) => void
}

/** How the last copy went: nothing yet (or long enough ago), put on the clipboard, or refused by it. */
export type Copied = 'idle' | 'copied' | 'refused'
/** How long "Copied" is said before the button says Copy again, in milliseconds. */
export const COPIED_FOR = 2500

/** The two things the window shows. */
export type View = 'traces' | 'logs'
/** The column a narrow window shows (`ui-split-view`'s `column`). */
export type Pane = 'sidebar' | 'content' | 'detail'
/** A probe that answers, one that fails in the backend, and one that answers and then fails to render on the page. */
export type Probe = 'answer' | 'fail' | 'render'
/** Whose logs the Logs view shows: every source's, or one source's (the host writes much that is not an app's). */
export type SourceFilter = 'all' | Source
/** What is picked in a trace: one of its spans, or one of its logs (by `logKey`). */
export type Choice = { kind: 'span'; id: string } | { kind: 'log'; key: string } | null

/** The host page does not offer telemetry to this frame. */
const NOT_OFFERED = -32601
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
const codeOf = (e: unknown): unknown => (e as { code?: unknown } | null)?.code

/** What the render that fails says: the failure NEOC-520 was, a keyed list given the same key twice. */
export const RENDER_FAILURE = 'each_key_duplicate: Keyed each block has duplicate key `nginx.service` at indexes 0 and 1'

const NO_LOGS: readonly Log[] = []

export class Telemetry {
  #traces = $state.raw<readonly TraceSummary[]>([])
  #trace = $state.raw<Found | null>(null)
  #logs = $state.raw<readonly Log[]>(NO_LOGS)
  #loaded = $state(false)
  #unavailable = $state<string | null>(null)
  #failure = $state<string | null>(null)
  #view = $state<View>('traces')
  #pane = $state<Pane>('content')
  #selected = $state<string | null>(null)
  #choice = $state<Choice>(null)
  #entry = $state<string | null>(null)
  #query = $state('')
  #level = $state<Level>('info')
  #source = $state<SourceFilter>('all')
  #failedOnly = $state(false)
  #probing = $state<Probe | null>(null)
  #copied = $state<Copied>('idle')
  #copies = 0
  #refreshes = 0

  readonly #reader: TelemetryReader
  readonly #rpc: ProbeRpc
  readonly #later: (fn: () => void) => void
  readonly #tracer: Tracer
  readonly #clipboard: Clipboard | null
  readonly #after: (fn: () => void, ms: number) => void

  constructor(reader: TelemetryReader, rpc: ProbeRpc, options: TelemetryOptions = {}) {
    this.#reader = reader
    this.#rpc = rpc
    this.#later = options.later ?? ((fn) => void setTimeout(fn))
    this.#tracer = options.tracer ?? untraced
    this.#clipboard = options.clipboard ?? null
    this.#after = options.after ?? ((fn, ms) => void setTimeout(fn, ms))
  }

  get loaded(): boolean {
    return this.#loaded
  }
  /** Why the window cannot read anything at all (the host page does not offer telemetry). */
  get unavailable(): string | null {
    return this.#unavailable
  }
  /** Why the last read failed. */
  get failure(): string | null {
    return this.#failure
  }
  get view(): View {
    return this.#view
  }
  get pane(): Pane {
    return this.#pane
  }
  get query(): string {
    return this.#query
  }
  get level(): Level {
    return this.#level
  }
  get source(): SourceFilter {
    return this.#source
  }
  /** The Traces view shows only the traces where something failed (a poll every few seconds buries them otherwise). */
  get failedOnly(): boolean {
    return this.#failedOnly
  }
  get probing(): Probe | null {
    return this.#probing
  }
  /** How the last copy of a log or a trace went. */
  get copied(): Copied {
    return this.#copied
  }
  get traceCount(): number {
    return this.#traces.length
  }
  /** The traces that match the search (and failed, when only those are shown), newest first. */
  get traces(): readonly TraceSummary[] {
    const found = matchingTraces(this.#traces, this.#query)
    return this.#failedOnly ? found.filter((t) => t.errors > 0) : found
  }
  /** The logs that match the search and the source, newest first. */
  get logs(): readonly Log[] {
    const found = matchingLogs(this.#logs, this.#query)
    const source = this.#source
    return source === 'all' ? found : found.filter((l) => l.source === source)
  }
  get selected(): string | null {
    return this.#selected
  }
  /** The open trace's summary, while it is in the list. */
  get summary(): TraceSummary | null {
    return this.#traces.find((t) => t.traceId === this.#selected) ?? null
  }
  /** The open trace's spans as a waterfall. */
  get bars(): Bar[] {
    return waterfall(this.#trace?.spans ?? [])
  }
  /** The open trace's logs, oldest first. */
  get traceLogs(): readonly Log[] {
    return [...(this.#trace?.logs ?? NO_LOGS)].sort((a, b) => a.timeMs - b.timeMs)
  }
  get choice(): Choice {
    return this.#choice
  }
  /** The log picked in the open trace. */
  get pickedLog(): Log | null {
    const choice = this.#choice
    return choice?.kind === 'log' ? (this.traceLogs.find((l) => logKey(l) === choice.key) ?? null) : null
  }
  /** The span picked in the open trace: the one picked, or the one the picked log was written in. */
  get pickedSpan(): Span | null {
    const choice = this.#choice
    const id = choice?.kind === 'span' ? choice.id : this.pickedLog?.spanId
    return id ? (this.#trace?.spans.find((s) => s.spanId === id) ?? null) : null
  }
  /** The log open in the Logs view. */
  get entry(): Log | null {
    return this.#logs.find((l) => logKey(l) === this.#entry) ?? null
  }

  /**
   * Read what the view shows again. Never rejects: a failure is kept, to be shown. A read that is overtaken by a later one
   * (a trace opened, a view shown, a poll) gives nothing: what it read may be of what is no longer on show, and the later
   * read is at least as new.
   */
  async refresh(): Promise<void> {
    const run = ++this.#refreshes
    try {
      if (this.#view === 'traces') {
        const traces = await this.#reader.traces()
        const selected = this.#selected
        const trace = selected ? await this.#reader.query({ traceId: selected, limit: 1000 }) : null
        if (run !== this.#refreshes) return
        this.#traces = traces
        if (trace) this.#trace = trace
      } else {
        const found = await this.#reader.query({ severity: this.#level, limit: 300 })
        if (run !== this.#refreshes) return
        this.#logs = [...found.logs].reverse()
      }
      this.#failure = null
      this.#unavailable = null
    } catch (e) {
      if (run !== this.#refreshes) return
      // a poll that fails again, the same way, is the same failure: it is warned of when it starts or changes, not on each tick
      if (codeOf(e) === NOT_OFFERED) {
        const said = 'This host page does not offer telemetry to this app.'
        if (this.#unavailable !== said) this.#tracer.warn('this host page does not offer telemetry', { error: messageOf(e) })
        this.#unavailable = said
      } else {
        const said = messageOf(e)
        if (this.#failure !== said) this.#tracer.warn('could not read the telemetry', { error: said, view: this.#view })
        this.#failure = said
      }
    } finally {
      // an overtaken read has told nothing: the page is loaded when the latest read has answered
      if (run === this.#refreshes) this.#loaded = true
    }
  }

  show(view: View): void {
    if (view !== this.#view) this.#query = ''
    this.#forgetCopy()
    this.#view = view
    this.#pane = 'content'
    void this.refresh()
  }

  /** Open a trace in the detail column, from the list or from a log. */
  async open(traceId: string): Promise<void> {
    this.#view = 'traces'
    this.#forgetCopy()
    this.#selected = traceId
    this.#choice = null
    this.#trace = null
    this.#pane = 'detail'
    await this.refresh()
  }

  /** Open a log of the Logs view in the detail column. */
  inspect(log: Log): void {
    this.#forgetCopy()
    this.#entry = logKey(log)
    this.#pane = 'detail'
  }

  /** Pick a span or a log of the open trace; picking it again lets it go. */
  choose(pick: NonNullable<Choice>): void {
    const now = this.#choice
    const same = now?.kind === pick.kind && (now.kind === 'span' ? now.id === (pick as { id: string }).id : now.key === (pick as { key: string }).key)
    this.#choice = same ? null : pick
  }

  /**
   * Put everything known of the open trace on the clipboard: its spans and logs, as JSON, read again now with the most
   * the host keeps (what the window shows is the last 1,000). One that could not be read again is copied as it was
   * loaded, and the copy says so.
   */
  async copyTrace(): Promise<void> {
    const traceId = this.#selected
    const loaded = this.#trace
    if (!traceId || !loaded) return
    // of this trace, as it is now: the window may show another by the time the host answers
    const summary = this.summary
    await this.#copy(async () => {
      try {
        const found = await this.#reader.query({ traceId, limit: EXPORT_LIMIT })
        return exportText(traceExport(traceId, found, summary, cutOff(found)))
      } catch (e) {
        this.#tracer.warn('could not read the trace again to copy it', { error: messageOf(e), traceId })
        // what the window had is a stand-in only if it is of this trace
        if (!loaded.spans.every((s) => s.traceId === traceId) || !loaded.logs.every((l) => l.traceId === traceId)) {
          throw new Error('The trace could not be read again, and the window has no copy of it.')
        }
        return exportText(traceExport(traceId, loaded, summary, `The trace could not be read again, so this is what the window had: ${messageOf(e)}`))
      }
    })
  }

  /**
   * Put everything known of a log on the clipboard, as JSON: the log, its span and, read now, the trace it belongs to
   * (the Logs view holds no trace). A trace that cannot be read is left out, and the copy says so.
   */
  async copyLog(log: Log): Promise<void> {
    await this.#copy(async () => {
      const traceId = log.traceId
      if (!traceId) return exportText(logExport(log, null, null))
      const summary = this.#traces.find((t) => t.traceId === traceId) ?? null
      try {
        const found = await this.#reader.query({ traceId, limit: EXPORT_LIMIT })
        return exportText(logExport(log, found, summary, cutOff(found)))
      } catch (e) {
        this.#tracer.warn('could not read the trace of the log to copy it', { error: messageOf(e), traceId })
        return exportText(logExport(log, null, summary, `The trace could not be read: ${messageOf(e)}`))
      }
    })
  }

  /** What was copied is no longer what is shown: an answer about it must not be given for what is shown now. */
  #forgetCopy(): void {
    this.#copies++
    this.#copied = 'idle'
  }

  /**
   * Copy what `text` makes; how it went is `copied` for a while, if what is shown has not changed by then. Never rejects.
   * `text` is called at once, in the click.
   */
  async #copy(text: () => Promise<string>): Promise<void> {
    const copy = ++this.#copies
    let answer: Copied
    try {
      if (!this.#clipboard) throw new Error('There is no clipboard.')
      await this.#clipboard.write(text())
      answer = 'copied'
    } catch (e) {
      this.#tracer.warn('could not copy to the clipboard', { error: messageOf(e) })
      answer = 'refused'
    }
    if (copy !== this.#copies) return
    this.#copied = answer
    // the answer stays for a while, unless another copy, or another thing on show, has come by then
    this.#after(() => {
      if (copy === this.#copies) this.#copied = 'idle'
    }, COPIED_FOR)
  }

  /** A narrow window goes back one column. */
  back(): void {
    this.#pane = this.#pane === 'detail' ? 'content' : 'sidebar'
  }

  search(text: string): void {
    this.#query = text
  }

  filter(level: Level): void {
    this.#level = level
    void this.refresh()
  }

  /** Show one source's logs, or every source's. */
  from(source: SourceFilter): void {
    this.#source = source
  }

  /** Show only the traces where something failed, or every trace. */
  onlyFailed(on: boolean): void {
    this.#failedOnly = on
  }

  /** Send a probe, in a span of its own; its trace shows once the page has sent what it recorded (every 2 s). */
  async probe(kind: Probe): Promise<void> {
    if (this.#probing) return
    this.#probing = kind
    try {
      await this.#tracer.span('probe', { kind }).in_scope(() => this.#rpc.probe({ fail: kind === 'fail' }))
      if (kind === 'render') {
        this.#later(() => {
          throw new Error(RENDER_FAILURE)
        })
      }
    } catch {
      // a failing probe is the point: its failure is in its trace
    } finally {
      this.#probing = null
    }
  }
}
