import { describe, expect, test } from 'bun:test'
import { attributeRows, duration, matchingLogs, matchingTraces, waterfall, type Span } from './telemetry'
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
