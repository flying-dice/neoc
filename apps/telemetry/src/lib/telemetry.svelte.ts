// One Telemetry window's state: the traces and logs it shows, what is picked, what is searched for, and the probes it
// sends. What it needs from outside arrives through the constructor (the reader, the backend, a way to run work later),
// so a test or a story builds one with fakes.
import {
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

/** Where what is copied goes (the page's `navigator.clipboard`, which the app's frame is allowed to write to). */
export interface Clipboard {
  writeText(text: string): Promise<void>
}

/** The backend's one method: a call to try tracing with (the app's private `_probe` on the bus: `App.svelte` sends it). */
export interface ProbeRpc {
  probe(params: VosApp.Telemetry.ProbeParams): Promise<VosApp.Telemetry.ProbeResult>
}

/** What the window traces its own work with (`vos.tracing`'s `span`): a probe is a span, and its call a child of it. */
export interface Tracer {
  span(name: string, fields?: { readonly [key: string]: unknown }): { in_scope<T>(body: () => T): T }
}

/** No tracing at all (a test, a story): the work runs as it is. */
export const untraced: Tracer = { span: () => ({ in_scope: (body) => body() }) }

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

  /** Read what the view shows again. Never rejects: a failure is kept, to be shown. */
  async refresh(): Promise<void> {
    try {
      if (this.#view === 'traces') {
        this.#traces = await this.#reader.traces()
        const selected = this.#selected
        if (selected) this.#trace = await this.#reader.query({ traceId: selected, limit: 1000 })
      } else {
        const found = await this.#reader.query({ severity: this.#level, limit: 300 })
        this.#logs = [...found.logs].reverse()
      }
      this.#failure = null
      this.#unavailable = null
    } catch (e) {
      if (codeOf(e) === NOT_OFFERED) this.#unavailable = 'This host page does not offer telemetry to this app.'
      else this.#failure = messageOf(e)
    } finally {
      this.#loaded = true
    }
  }

  show(view: View): void {
    if (view !== this.#view) this.#query = ''
    this.#view = view
    this.#pane = 'content'
    void this.refresh()
  }

  /** Open a trace in the detail column, from the list or from a log. */
  async open(traceId: string): Promise<void> {
    this.#view = 'traces'
    this.#copied = 'idle'
    this.#selected = traceId
    this.#choice = null
    this.#trace = null
    this.#pane = 'detail'
    await this.refresh()
  }

  /** Open a log of the Logs view in the detail column. */
  inspect(log: Log): void {
    this.#copied = 'idle'
    this.#entry = logKey(log)
    this.#pane = 'detail'
  }

  /** Pick a span or a log of the open trace; picking it again lets it go. */
  choose(pick: NonNullable<Choice>): void {
    const now = this.#choice
    const same = now?.kind === pick.kind && (now.kind === 'span' ? now.id === (pick as { id: string }).id : now.key === (pick as { key: string }).key)
    this.#choice = same ? null : pick
  }

  /** Put everything known of the open trace on the clipboard: its spans and logs, as JSON. */
  async copyTrace(): Promise<void> {
    const traceId = this.#selected
    const found = this.#trace
    if (!traceId || !found) return
    await this.#copy(() => exportText(traceExport(traceId, found, this.summary)))
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
        return exportText(logExport(log, await this.#reader.query({ traceId, limit: 1000 }), summary))
      } catch (e) {
        return exportText(logExport(log, null, summary, `The trace could not be read: ${messageOf(e)}`))
      }
    })
  }

  /** Copy what `text` makes; how it went is `copied` for a while. Never rejects. */
  async #copy(text: () => string | Promise<string>): Promise<void> {
    const copy = ++this.#copies
    try {
      if (!this.#clipboard) throw new Error('There is no clipboard.')
      await this.#clipboard.writeText(await text())
      this.#copied = 'copied'
    } catch {
      this.#copied = 'refused'
    }
    // the answer stays for a while, unless another copy has been made by then
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
