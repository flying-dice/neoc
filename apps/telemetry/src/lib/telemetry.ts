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

const isoOf = (ms: number): string => new Date(ms).toISOString()

/** A log as it is copied: its time as ISO text and as milliseconds, its message, where it was written, and every field. */
function logReport(log: Log): Record<string, unknown> {
  return {
    time: isoOf(log.timeMs),
    timeMs: log.timeMs,
    severity: log.severity,
    message: log.body,
    source: log.source,
    app: log.app ?? null,
    traceId: log.traceId ?? null,
    spanId: log.spanId ?? null,
    attributes: log.attributes
  }
}

/** A span as it is copied: its place in the waterfall, its timing as ISO text and milliseconds, its outcome and every field. */
function spanReport(span: Span, depth: number): Record<string, unknown> {
  return {
    name: span.name,
    spanId: span.spanId,
    parentSpanId: span.parentSpanId ?? null,
    depth,
    source: span.source,
    app: span.app ?? null,
    status: span.status,
    start: isoOf(span.startMs),
    startMs: span.startMs,
    endMs: span.endMs,
    durationMs: span.endMs - span.startMs,
    attributes: span.attributes
  }
}

/**
 * Everything the window knows of one trace, as plain data to copy: how it went, every span in waterfall order with its
 * depth, timing and fields, and every log, oldest first. `summary` is the trace's row of the list, when it is still there.
 */
export function traceReport(traceId: string, found: Found, summary: TraceSummary | null): Record<string, unknown> {
  const bars = waterfall(found.spans)
  const logs = [...found.logs].sort((a, b) => a.timeMs - b.timeMs)
  const start = bars.length > 0 ? Math.min(...bars.map((b) => b.span.startMs)) : null
  const end = bars.length > 0 ? Math.max(...bars.map((b) => b.span.endMs)) : null
  return {
    traceId,
    name: summary?.name ?? bars[0]?.span.name ?? null,
    app: summary?.app ?? bars[0]?.span.app ?? null,
    start: start === null ? null : isoOf(start),
    durationMs: start === null || end === null ? null : end - start,
    failed: found.spans.filter((s) => s.status === 'error').length + logs.filter((l) => l.severity === 'error').length,
    spans: bars.map((b) => spanReport(b.span, b.depth)),
    logs: logs.map(logReport)
  }
}

/**
 * Everything the window knows of one log, as plain data to copy: the log, the span it was written in, and the trace it
 * belongs to in full (`trace` is null for a log written outside any call, or when the trace could not be read: `note`
 * says so).
 */
export function logExport(log: Log, trace: Found | null, summary: TraceSummary | null, note?: string): Record<string, unknown> {
  const span = log.spanId ? (trace?.spans.find((s) => s.spanId === log.spanId) ?? null) : null
  const depth = span ? (waterfall(trace?.spans ?? []).find((b) => b.span.spanId === span.spanId)?.depth ?? 0) : 0
  return {
    kind: 'log',
    log: logReport(log),
    span: span ? spanReport(span, depth) : null,
    trace: log.traceId && trace ? traceReport(log.traceId, trace, summary) : null,
    ...(note ? { note } : {})
  }
}

/** A trace's export: its report, said to be a trace's. */
export const traceExport = (traceId: string, found: Found, summary: TraceSummary | null): Record<string, unknown> => ({
  kind: 'trace',
  ...traceReport(traceId, found, summary)
})

/** An export as the text that is copied: JSON, indented, so a person can read it as well as a program. */
export const exportText = (report: Record<string, unknown>): string => JSON.stringify(report, null, 2)
