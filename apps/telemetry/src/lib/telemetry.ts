// What the host recorded (spans and logs, shaped after OpenTelemetry), and the pure work on it: the waterfall of a trace,
// what matches a search, and how a time or a duration is written. NEOC-522 proof of concept.

export type Source = 'browser' | 'host' | 'backend'
export type Severity = 'trace' | 'debug' | 'info' | 'warn' | 'error'

export interface Span {
  traceId: string
  spanId: string
  parentSpanId?: string
  name: string
  source: Source
  app?: string
  startMs: number
  endMs: number
  status: 'ok' | 'error' | 'unset'
  attributes: Record<string, unknown>
}

export interface Log {
  timeMs: number
  severity: Severity
  body: string
  source: Source
  app?: string
  traceId?: string
  spanId?: string
  attributes: Record<string, unknown>
}

export interface TraceSummary {
  traceId: string
  /** The root span's name. */
  name: string
  app?: string
  startMs: number
  durationMs: number
  spans: number
  errors: number
}

export interface Found {
  spans: Span[]
  logs: Log[]
}

export interface LogQuery {
  traceId?: string
  app?: string
  severity?: Severity
  limit?: number
}

/** The lowest severity the Logs view shows. */
export type Level = 'debug' | 'info' | 'warn' | 'error'

/** The tint each source is drawn in, the same in the waterfall and the logs. */
export const SOURCE_TINT: Readonly<Record<Source, string>> = {
  browser: 'var(--ios-blue)',
  host: 'var(--ios-purple)',
  backend: 'var(--ios-teal)'
}

export const SOURCE_LABEL: Readonly<Record<Source, string>> = { browser: 'Page', host: 'Host', backend: 'Backend' }

export const SEVERITY_LABEL: Readonly<Record<Severity, string>> = {
  trace: 'Trace',
  debug: 'Debug',
  info: 'Info',
  warn: 'Warning',
  error: 'Error'
}

/** A log has no id of its own: when it was written, where, and what it says tells it apart. */
export const logKey = (log: Log): string => `${log.timeMs}:${log.spanId ?? ''}:${log.source}:${log.body}`

/** The traces whose root or app has the text in it. */
export function matchingTraces(traces: readonly TraceSummary[], query: string): readonly TraceSummary[] {
  const text = query.trim().toLowerCase()
  if (text === '') return traces
  return traces.filter((t) => t.name.toLowerCase().includes(text) || (t.app ?? '').toLowerCase().includes(text))
}

/** The logs whose message or app has the text in it. */
export function matchingLogs(logs: readonly Log[], query: string): readonly Log[] {
  const text = query.trim().toLowerCase()
  if (text === '') return logs
  return logs.filter((l) => l.body.toLowerCase().includes(text) || (l.app ?? '').toLowerCase().includes(text))
}

/** One row of a waterfall: the span, how deep it sits, and where its bar starts and how wide it is (0..1 of the trace). */
export interface Bar {
  span: Span
  depth: number
  left: number
  width: number
}

/**
 * The spans of one trace as a waterfall: parents before their children (a span whose parent was not recorded is a root),
 * siblings by start, bars placed on the trace's own time line. The page's clock and the host's may disagree by a few
 * milliseconds; a bar is clamped into the line rather than drawn outside it.
 */
export function waterfall(spans: readonly Span[]): Bar[] {
  if (spans.length === 0) return []
  const start = Math.min(...spans.map((s) => s.startMs))
  const end = Math.max(...spans.map((s) => s.endMs))
  const total = Math.max(end - start, 1)
  const ids = new Set(spans.map((s) => s.spanId))
  const children = new Map<string, Span[]>()
  const roots: Span[] = []
  for (const s of spans) {
    if (s.parentSpanId && ids.has(s.parentSpanId) && s.parentSpanId !== s.spanId) {
      const list = children.get(s.parentSpanId) ?? []
      list.push(s)
      children.set(s.parentSpanId, list)
    } else roots.push(s)
  }
  const byStart = (a: Span, b: Span): number => a.startMs - b.startMs
  const out: Bar[] = []
  const seen = new Set<string>()
  const walk = (s: Span, depth: number): void => {
    if (seen.has(s.spanId)) return
    seen.add(s.spanId)
    const left = Math.min(Math.max((s.startMs - start) / total, 0), 1)
    const width = Math.min(Math.max((s.endMs - s.startMs) / total, 0.01), 1 - left)
    out.push({ span: s, depth, left, width })
    for (const c of (children.get(s.spanId) ?? []).sort(byStart)) walk(c, depth + 1)
  }
  for (const r of [...roots].sort(byStart)) walk(r, 0)
  return out
}

export function duration(ms: number): string {
  if (ms < 1) return `${Math.max(Math.round(ms * 1000), 1)} µs`
  if (ms < 1000) return `${ms.toFixed(ms < 10 ? 1 : 0)} ms`
  return `${(ms / 1000).toFixed(2)} s`
}

/** A wall-clock time to the millisecond, as the logs are read. */
export function clock(ms: number): string {
  const d = new Date(ms)
  const pad = (n: number, w = 2): string => String(n).padStart(w, '0')
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`
}

/** An attribute's value as one line of text. */
export function attributeText(value: unknown): string {
  if (value === null || value === undefined) return '—'
  if (typeof value === 'string') return value
  return JSON.stringify(value)
}

/** The attributes worth showing, in name order; the record's own bookkeeping (`target`) last. */
export function attributeRows(attributes: Record<string, unknown>): [string, string][] {
  return Object.entries(attributes)
    .filter(([, v]) => v !== undefined)
    .sort(([a], [b]) => (a === 'target' ? 1 : b === 'target' ? -1 : a.localeCompare(b)))
    .map(([k, v]) => [k, attributeText(v)])
}
