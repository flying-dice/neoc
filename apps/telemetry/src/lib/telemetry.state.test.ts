import { describe, expect, test } from 'bun:test'
import { COPIED_FOR, Telemetry, RENDER_FAILURE, type Clipboard } from './telemetry.svelte'
import { FAILED_TRACE, LOGS, RENDER_TRACE, SPANS, createFakeBackend, rpcError } from './fakeApi'

const window = (options: Parameters<typeof createFakeBackend>[0] = {}, later?: (fn: () => void) => void) => {
  const backend = createFakeBackend(options)
  return { backend, telemetry: new Telemetry(backend.reader, backend.rpc, { later }) }
}

describe('Telemetry', () => {
  test('the traces are read newest first, and a search narrows them', async () => {
    const { telemetry } = window()
    await telemetry.refresh()
    expect(telemetry.loaded).toBe(true)
    expect(telemetry.traces.map((t) => t.traceId)).toEqual([FAILED_TRACE, RENDER_TRACE])
    telemetry.search('overview')
    expect(telemetry.traces.map((t) => t.traceId)).toEqual([RENDER_TRACE])
  })

  test('opening a trace draws its waterfall; picking a log marks the span it was written in, and again lets it go', async () => {
    const { telemetry } = window()
    await telemetry.open(RENDER_TRACE)
    expect(telemetry.pane).toBe('detail')
    expect(telemetry.bars.map((b) => b.span.name)).toEqual(['rpc overview', 'app.call', 'process.run', 'log.info'])
    const error = telemetry.traceLogs.find((l) => l.severity === 'error')!
    telemetry.choose({ kind: 'log', key: `${error.timeMs}:${error.spanId}:${error.source}:${error.body}` })
    expect(telemetry.pickedLog?.body).toContain('each_key_duplicate')
    expect(telemetry.pickedSpan?.name).toBe('rpc overview')
    telemetry.choose({ kind: 'log', key: `${error.timeMs}:${error.spanId}:${error.source}:${error.body}` })
    expect(telemetry.pickedLog).toBeNull()
  })

  test('only the traces where something failed can be listed, and the logs of one source', async () => {
    const healthy = { ...SPANS[0]!, traceId: '0123456789abcdef0123456789abcdef', spanId: '7777777777777777', name: 'rpc _overview', startMs: 9000, endMs: 9003 }
    const { telemetry } = window({ spans: [...SPANS, healthy], logs: LOGS })
    await telemetry.refresh()
    expect(telemetry.traces).toHaveLength(3)
    telemetry.onlyFailed(true)
    expect(telemetry.traces.map((t) => t.traceId)).toEqual([FAILED_TRACE, RENDER_TRACE])
    telemetry.show('logs')
    await telemetry.refresh()
    telemetry.from('browser')
    expect(telemetry.logs.map((l) => l.source)).toEqual(['browser'])
    telemetry.from('all')
    expect(new Set(telemetry.logs.map((l) => l.source))).toEqual(new Set(['browser', 'backend']))
  })

  test('the Logs view reads at the level chosen, and a log opens in the detail column', async () => {
    const { telemetry } = window()
    telemetry.show('logs')
    await telemetry.refresh()
    expect(telemetry.logs).toHaveLength(4)
    telemetry.filter('error')
    await telemetry.refresh()
    expect(telemetry.logs.every((l) => l.severity === 'error')).toBe(true)
    telemetry.inspect(telemetry.logs[0]!)
    expect(telemetry.entry?.severity).toBe('error')
    expect(telemetry.pane).toBe('detail')
  })

  test('a host page that does not offer telemetry makes the window unavailable; any other failure is kept to be shown', async () => {
    const off = window({ failWith: rpcError(-32601, 'This host does not offer vos/telemetry/query') }).telemetry
    await off.refresh()
    expect(off.unavailable).not.toBeNull()
    const broken = window({ failWith: rpcError(-32000, 'The host went away') }).telemetry
    await broken.refresh()
    expect([broken.unavailable, broken.failure]).toEqual([null, 'The host went away'])
  })

  test('a probe calls the backend; a failing one is not the window’s failure; the render probe fails outside the call', async () => {
    const later: (() => void)[] = []
    const { backend, telemetry } = window({}, (fn) => later.push(fn))
    await telemetry.probe('answer')
    await telemetry.probe('fail')
    await telemetry.probe('render')
    expect(backend.probes).toEqual([false, true, false])
    expect(telemetry.failure).toBeNull()
    expect(later).toHaveLength(1)
    expect(() => later[0]!()).toThrow(RENDER_FAILURE)
  })

  test('a probe is a span of its own, whatever its call did', async () => {
    const opened: [string, unknown][] = []
    const backend = createFakeBackend()
    const tracer = { span: (name: string, fields?: unknown) => (opened.push([name, fields]), { in_scope: <T,>(body: () => T) => body() }) }
    const telemetry = new Telemetry(backend.reader, backend.rpc, { tracer })
    await telemetry.probe('fail')
    expect(opened).toEqual([['probe', { kind: 'fail' }]])
  })

  test('a narrow window goes back a column at a time', async () => {
    const { telemetry } = window()
    await telemetry.open(RENDER_TRACE)
    telemetry.back()
    expect(telemetry.pane).toBe('content')
    telemetry.back()
    expect(telemetry.pane).toBe('sidebar')
  })
})

describe('copying what is known of a trace or a log', () => {
  /** A window whose clipboard keeps what it was given, and whose waits are run by the test. */
  const copying = (options: Parameters<typeof createFakeBackend>[0] = {}, clipboard?: Clipboard) => {
    const written: string[] = []
    const waits: [() => void, number][] = []
    const backend = createFakeBackend(options)
    const telemetry = new Telemetry(backend.reader, backend.rpc, {
      clipboard: clipboard ?? { writeText: async (text) => void written.push(text) },
      after: (fn, ms) => void waits.push([fn, ms])
    })
    return { backend, telemetry, written, waits }
  }

  test('the open trace is copied as JSON, and the button says Copied until the wait is over', async () => {
    const { telemetry, written, waits } = copying()
    await telemetry.open(RENDER_TRACE)
    expect(telemetry.copied).toBe('idle')
    await telemetry.copyTrace()
    expect(telemetry.copied).toBe('copied')
    const report = JSON.parse(written[0]!) as { kind: string; traceId: string; name: string; spans: unknown[]; logs: unknown[] }
    expect([report.kind, report.traceId, report.name, report.spans.length, report.logs.length]).toEqual(['trace', RENDER_TRACE, 'rpc overview', 4, 2])
    expect(waits.map(([, ms]) => ms)).toEqual([COPIED_FOR])
    waits[0]![0]()
    expect(telemetry.copied).toBe('idle')
  })

  test('a log of the Logs view is copied with its span and the trace it belongs to, which is read for it', async () => {
    const { telemetry, written } = copying()
    telemetry.show('logs')
    await telemetry.refresh()
    const error = telemetry.logs.find((l) => l.body.includes('each_key_duplicate'))!
    await telemetry.copyLog(error)
    expect(telemetry.copied).toBe('copied')
    const report = JSON.parse(written[0]!) as { kind: string; log: { message: string }; span: { name: string }; trace: { spans: unknown[]; logs: unknown[] } }
    expect(report.kind).toBe('log')
    expect(report.log.message).toContain('each_key_duplicate')
    expect(report.span.name).toBe('rpc overview')
    expect([report.trace.spans.length, report.trace.logs.length]).toEqual([4, 2])
  })

  test('a log written outside any call is copied alone, and one whose trace cannot be read says so and is still copied', async () => {
    const lone = { ...LOGS[0]!, traceId: undefined, spanId: undefined }
    const alone = copying()
    await alone.telemetry.copyLog(lone)
    expect(JSON.parse(alone.written[0]!)).toMatchObject({ kind: 'log', span: null, trace: null })
    const broken = copying()
    await broken.telemetry.refresh()
    broken.backend.reader.query = () => Promise.reject(new Error('The host went away'))
    await broken.telemetry.copyLog(LOGS[1]!)
    expect(broken.telemetry.copied).toBe('copied')
    expect(JSON.parse(broken.written[0]!)).toMatchObject({ trace: null, note: 'The trace could not be read: The host went away' })
  })

  test('a clipboard that refuses, or none at all, is said, never raised', async () => {
    const refusing = copying({}, { writeText: () => Promise.reject(new DOMException('denied', 'NotAllowedError')) })
    await refusing.telemetry.open(RENDER_TRACE)
    await refusing.telemetry.copyTrace()
    expect(refusing.telemetry.copied).toBe('refused')
    const none = new Telemetry(createFakeBackend().reader, createFakeBackend().rpc)
    await none.copyLog(LOGS[1]!)
    expect(none.copied).toBe('refused')
  })

  test('an older copy’s wait does not end a newer copy’s Copied; opening something else clears it; nothing to copy copies nothing', async () => {
    const { telemetry, written, waits } = copying()
    await telemetry.copyTrace()
    expect(written).toEqual([])
    await telemetry.open(RENDER_TRACE)
    await telemetry.copyTrace()
    await telemetry.copyTrace()
    waits[0]![0]()
    expect(telemetry.copied).toBe('copied')
    waits[1]![0]()
    expect(telemetry.copied).toBe('idle')
    await telemetry.copyTrace()
    await telemetry.open(FAILED_TRACE)
    expect(telemetry.copied).toBe('idle')
  })
})
