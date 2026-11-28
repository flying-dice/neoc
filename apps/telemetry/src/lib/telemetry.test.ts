import { describe, expect, test } from 'bun:test'
import { attributeRows, duration, exportText, logExport, matchingLogs, matchingTraces, traceExport, waterfall, type Span } from './telemetry'
import { FAILED_TRACE, LOGS, RENDER_TRACE, SPANS } from './fakeApi'

describe('waterfall', () => {
  test('parents come before their children, each a level deeper, on the trace’s own time line', () => {
    const bars = waterfall(SPANS.filter((s) => s.traceId === RENDER_TRACE))
    expect(bars.map((b) => [b.span.name, b.depth])).toEqual([
      ['rpc overview', 0],
      ['app.call', 1],
      ['process.run', 2],
      ['log.info', 2]
    ])
    expect(bars[0]!.left).toBe(0)
    expect(bars[0]!.width).toBe(1)
    for (const b of bars) expect(b.left + b.width).toBeLessThanOrEqual(1)
  })

  test('a span whose parent was not recorded is a root, and a cycle does not loop', () => {
    const lone: Span = { ...SPANS[1]!, parentSpanId: 'ffffffffffffffff' }
    const self: Span = { ...SPANS[0]!, parentSpanId: SPANS[0]!.spanId }
    expect(waterfall([lone, self]).map((b) => b.depth)).toEqual([0, 0])
    expect(waterfall([])).toEqual([])
  })
})

test('a search matches a trace’s root or app, and a log’s message or app', () => {
  const traces = [
    { traceId: RENDER_TRACE, name: 'rpc overview', app: 'services', startMs: 0, durationMs: 1, spans: 1, errors: 0 },
    { traceId: FAILED_TRACE, name: 'rpc restart', app: 'dns', startMs: 0, durationMs: 1, spans: 1, errors: 1 }
  ]
  expect(matchingTraces(traces, ' Restart ').map((t) => t.traceId)).toEqual([FAILED_TRACE])
  expect(matchingTraces(traces, 'dns').map((t) => t.traceId)).toEqual([FAILED_TRACE])
  expect(matchingTraces(traces, '')).toHaveLength(2)
  expect(matchingLogs(LOGS, 'each_key')).toHaveLength(1)
})

test('a duration is written in the unit it reads best in', () => {
  expect(duration(0.385)).toBe('385 µs')
  expect(duration(7.12)).toBe('7.1 ms')
  expect(duration(42)).toBe('42 ms')
  expect(duration(1500)).toBe('1.50 s')
})

test('attributes are listed by name, the record’s target last, values as text', () => {
  expect(attributeRows({ target: 'vos_lua::vm', units: 3, b: { x: 1 }, a: 'yes' })).toEqual([
    ['a', 'yes'],
    ['b', '{"x":1}'],
    ['units', '3'],
    ['target', 'vos_lua::vm']
  ])
})

describe('what is copied of a trace', () => {
  const found = { spans: SPANS.filter((s) => s.traceId === RENDER_TRACE), logs: LOGS.filter((l) => l.traceId === RENDER_TRACE) }

  test('is its spans in waterfall order with their depth, timing and fields, and its logs oldest first', () => {
    const report = traceExport(RENDER_TRACE, { spans: [...found.spans].reverse(), logs: [...found.logs].reverse() }, null) as {
      kind: string
      name: string
      app: string
      durationMs: number
      failed: number
      spans: { name: string; depth: number; parentSpanId: string | null; durationMs: number; start: string; attributes: Record<string, unknown> }[]
      logs: { message: string; time: string; attributes: Record<string, unknown> }[]
    }
    expect(report.kind).toBe('trace')
    expect(report.name).toBe('rpc overview')
    expect(report.app).toBe('services')
    expect(report.durationMs).toBe(42)
    // the error log of the page
    expect(report.failed).toBe(1)
    expect(report.spans.map((s) => [s.name, s.depth])).toEqual([
      ['rpc overview', 0],
      ['app.call', 1],
      ['process.run', 2],
      ['log.info', 2]
    ])
    expect(report.spans[0]!.parentSpanId).toBeNull()
    expect(report.spans[1]!.parentSpanId).toBe('1111111111111111')
    expect(report.spans[0]!.attributes).toEqual({ 'rpc.method': 'overview' })
    expect(report.spans[0]!.start).toBe(new Date(SPANS[0]!.startMs).toISOString())
    expect(report.logs.map((l) => l.message)).toEqual(['read the overview', expect.stringContaining('each_key_duplicate')])
    expect(report.logs[1]!.attributes).toEqual({ 'exception.type': 'Error', 'vos.correlated': 'last-call' })
  })

  test('says what the list says of it when it is given the row, and is JSON a program can read back', () => {
    const summary = { traceId: RENDER_TRACE, name: 'the row', app: 'rowapp', startMs: 0, durationMs: 1, spans: 4, errors: 1 }
    const report = JSON.parse(exportText(traceExport(RENDER_TRACE, found, summary))) as { name: string; app: string; traceId: string }
    expect([report.name, report.app, report.traceId]).toEqual(['the row', 'rowapp', RENDER_TRACE])
  })

  test('a trace with nothing in it is still a report', () => {
    const report = traceExport('abc', { spans: [], logs: [] }, null) as { name: unknown; start: unknown; spans: unknown[]; logs: unknown[] }
    expect([report.name, report.start, report.spans, report.logs]).toEqual([null, null, [], []])
  })
})

describe('what is copied of a log', () => {
  const error = LOGS[1]!
  const found = { spans: SPANS.filter((s) => s.traceId === RENDER_TRACE), logs: LOGS.filter((l) => l.traceId === RENDER_TRACE) }

  test('is the log with every field, the span it was written in, and its whole trace', () => {
    const report = logExport(error, found, null) as {
      kind: string
      log: { message: string; severity: string; time: string; traceId: string; spanId: string; attributes: Record<string, unknown> }
      span: { name: string; depth: number }
      trace: { traceId: string; spans: unknown[]; logs: unknown[] }
    }
    expect(report.kind).toBe('log')
    expect(report.log.message).toContain('each_key_duplicate')
    expect(report.log.severity).toBe('error')
    expect(report.log.time).toBe(new Date(error.timeMs).toISOString())
    expect(report.log.attributes).toEqual({ 'exception.type': 'Error', 'vos.correlated': 'last-call' })
    expect([report.span.name, report.span.depth]).toEqual(['rpc overview', 0])
    expect(report.trace.traceId).toBe(RENDER_TRACE)
    expect([report.trace.spans.length, report.trace.logs.length]).toEqual([4, 2])
  })

  test('that was written outside any call has no span and no trace; a trace that could not be read says so', () => {
    const lone = { ...error, traceId: undefined, spanId: undefined }
    expect(logExport(lone, null, null)).toMatchObject({ span: null, trace: null })
    expect(logExport(lone, null, null)).not.toHaveProperty('note')
    expect(logExport(error, null, null, 'The trace could not be read: gone')).toMatchObject({ span: null, trace: null, note: 'The trace could not be read: gone' })
  })
})
