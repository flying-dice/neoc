import { describe, expect, test } from 'bun:test'
import { EXPORT_LIMIT } from './telemetry'
import { COPIED_FOR, Telemetry, RENDER_FAILURE, type Clipboard } from './telemetry.svelte'
import { FAILED_TRACE, LOGS, RENDER_TRACE, SPANS, createFakeBackend, rpcError } from './fakeApi'

/** Wait until `done`, for a read that is not awaited. */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

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
    const tracer = { span: (name: string, fields?: unknown) => (opened.push([name, fields]), { in_scope: <T,>(body: () => T) => body() }), warn: () => {} }
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
      clipboard: clipboard ?? { write: async (text) => void written.push(await text) },
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
    const refusing = copying({}, { write: () => Promise.reject(new DOMException('denied', 'NotAllowedError')) })
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

  test('the write begins in the click, with the text still to come', async () => {
    let given: unknown
    const { telemetry } = copying({}, { write: (text) => ((given = text), text.then(() => {})) })
    await telemetry.open(RENDER_TRACE)
    const copy = telemetry.copyTrace()
    expect(given).toBeInstanceOf(Promise)
    await copy
    expect(telemetry.copied).toBe('copied')
  })

  test('a trace is read for the copy with the most the host keeps, and a copy that reached it says the oldest may be missing', async () => {
    const asked: (number | undefined)[] = []
    const { backend, telemetry, written } = copying()
    await telemetry.open(RENDER_TRACE)
    const query = backend.reader.query
    backend.reader.query = (q) => {
      asked.push(q.limit)
      return query(q)
    }
    await telemetry.copyTrace()
    expect(asked).toEqual([EXPORT_LIMIT])
    expect(JSON.parse(written[0]!)).not.toHaveProperty('note')

    const full = { spans: SPANS.slice(0, 1), logs: Array.from({ length: EXPORT_LIMIT }, () => LOGS[0]!) }
    backend.reader.query = () => Promise.resolve(full)
    await telemetry.copyTrace()
    expect((JSON.parse(written[1]!) as { note: string }).note).toContain('may be missing')
    telemetry.show('logs')
    await telemetry.copyLog(LOGS[1]!)
    expect((JSON.parse(written[2]!) as { note: string }).note).toContain('may be missing')
  })

  test('a trace that cannot be read again is copied as the window had it, and says so', async () => {
    const { backend, telemetry, written } = copying()
    await telemetry.open(RENDER_TRACE)
    backend.reader.query = () => Promise.reject(new Error('The host went away'))
    await telemetry.copyTrace()
    expect(telemetry.copied).toBe('copied')
    const report = JSON.parse(written[0]!) as { note: string; spans: unknown[] }
    expect([report.spans.length, report.note]).toEqual([4, 'The trace could not be read again, so this is what the window had: The host went away'])
  })

  test('a copy still being made does not report on what is shown by the time it is done', async () => {
    const { backend, telemetry, written, waits } = copying()
    telemetry.show('logs')
    await telemetry.refresh()
    const [first, second] = [LOGS[0]!, LOGS[1]!]
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => (release = resolve))
    const query = backend.reader.query
    backend.reader.query = async (q) => (await gate, query(q))
    const copying1 = telemetry.copyLog(first)
    telemetry.inspect(second)
    release()
    await copying1
    expect(written).toHaveLength(1)
    expect(telemetry.copied).toBe('idle')
    expect(waits).toEqual([])
  })

  test('a trace is copied with its own name and app, whichever trace is open by the time the host answers', async () => {
    const { backend, telemetry, written } = copying()
    await telemetry.open(RENDER_TRACE)
    const mine = telemetry.summary!
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => (release = resolve))
    const query = backend.reader.query
    backend.reader.query = async (q) => (q.limit === EXPORT_LIMIT ? (await gate, query(q)) : query(q))
    const copy = telemetry.copyTrace()
    await telemetry.open(FAILED_TRACE)
    const other = telemetry.summary!
    expect(other.name).not.toBe(mine.name)
    release()
    await copy
    const report = JSON.parse(written[0]!) as { traceId: string; name: string; app: string | null }
    expect([report.traceId, report.name, report.app]).toEqual([RENDER_TRACE, mine.name, mine.app ?? null])
    expect(telemetry.copied).toBe('idle')
  })

  test('reads of two traces that finish out of order leave the open trace its own, and a failed copy falls back on that', async () => {
    const { backend, telemetry, written } = copying()
    await telemetry.refresh()
    const query = backend.reader.query
    const answer: Record<string, () => void> = {}
    backend.reader.query = (q) => (q.limit === 1000 && q.traceId ? new Promise((resolve) => (answer[q.traceId!] = () => resolve(query(q)))) : query(q))
    const first = telemetry.open(RENDER_TRACE)
    await Bun.sleep(5)
    const second = telemetry.open(FAILED_TRACE)
    await Bun.sleep(5)
    answer[FAILED_TRACE]!()
    await second
    answer[RENDER_TRACE]!()
    await first
    expect(telemetry.selected).toBe(FAILED_TRACE)
    expect(telemetry.bars.length).toBeGreaterThan(0)
    expect(telemetry.bars.every((b) => b.span.traceId === FAILED_TRACE)).toBe(true)

    backend.reader.query = () => Promise.reject(new Error('The host went away'))
    await telemetry.copyTrace()
    const report = JSON.parse(written[0]!) as { traceId: string; name: string; spans: { name: string }[]; note: string }
    expect(report.traceId).toBe(FAILED_TRACE)
    expect(report.name).toBe(telemetry.summary!.name)
    expect(report.spans.length).toBe(telemetry.bars.length)
    expect(report.note).toContain('this is what the window had')
  })

  test('the page is not loaded until the latest read has answered, whatever an overtaken one does', async () => {
    const { backend, telemetry } = copying()
    const traces = backend.reader.traces
    const query = backend.reader.query
    let first: () => void = () => {}
    backend.reader.traces = () => new Promise((resolve) => (first = () => resolve([])))
    const slow = telemetry.refresh()
    let second: () => void = () => {}
    backend.reader.query = (q) => new Promise((resolve) => (second = () => resolve(query(q))))
    telemetry.show('logs')
    await Bun.sleep(5)
    first()
    await slow
    expect(telemetry.loaded).toBe(false)
    backend.reader.traces = traces
    second()
    await until(() => telemetry.loaded)
    expect(telemetry.loaded).toBe(true)
  })

  test('a read that is overtaken by a later one gives nothing, not a failure either', async () => {
    const { backend, telemetry } = copying()
    await telemetry.refresh()
    const traces = backend.reader.traces
    let fail: (e: Error) => void = () => {}
    backend.reader.traces = () => new Promise((_, reject) => (fail = reject))
    const slow = telemetry.refresh()
    backend.reader.traces = traces
    await telemetry.refresh()
    fail(new Error('too old to matter'))
    await slow
    expect(telemetry.failure).toBeNull()
  })

  test('a failure the window catches and shows is warned of, once, where it is caught', async () => {
    const warned: [string, unknown][] = []
    const tracer = { span: () => ({ in_scope: <T,>(body: () => T) => body() }), warn: (message: string, fields?: unknown) => void warned.push([message, fields]) }
    const given = (options: Parameters<typeof createFakeBackend>[0] = {}, clipboard?: Clipboard) => {
      const backend = createFakeBackend(options)
      return { backend, telemetry: new Telemetry(backend.reader, backend.rpc, { tracer, clipboard, after: () => {} }) }
    }
    const messages = () => warned.splice(0).map(([message]) => message)

    // a poll that goes on failing the same way is warned of once; a read that works re-arms it; another failure warns again
    const polled = given({ failWith: rpcError(-32000, 'The host went away') })
    await polled.telemetry.refresh()
    await polled.telemetry.refresh()
    await polled.telemetry.refresh()
    expect(messages()).toEqual(['could not read the telemetry'])
    const reader = polled.backend.reader.traces
    polled.backend.reader.traces = () => Promise.resolve([])
    await polled.telemetry.refresh()
    expect(polled.telemetry.failure).toBeNull()
    expect(messages()).toEqual([])
    polled.backend.reader.traces = reader
    await polled.telemetry.refresh()
    await polled.telemetry.refresh()
    expect(messages()).toEqual(['could not read the telemetry'])
    polled.backend.reader.traces = () => Promise.reject(rpcError(-32000, 'The host answered nothing'))
    await polled.telemetry.refresh()
    expect(messages()).toEqual(['could not read the telemetry'])

    const off = given({ failWith: rpcError(-32601, 'This host does not offer vos/telemetry/query') })
    await off.telemetry.refresh()
    await off.telemetry.refresh()
    expect(messages()).toEqual(['this host page does not offer telemetry'])

    const refused = given({}, { write: () => Promise.reject(new DOMException('denied', 'NotAllowedError')) })
    await refused.telemetry.open(RENDER_TRACE)
    await refused.telemetry.copyTrace()
    expect(warned).toEqual([['could not copy to the clipboard', { error: 'denied' }]])
    warned.length = 0

    const reading = given({}, { write: async (text) => void (await text) })
    await reading.telemetry.open(RENDER_TRACE)
    reading.backend.reader.query = () => Promise.reject(new Error('The host went away'))
    await reading.telemetry.copyTrace()
    await reading.telemetry.copyLog(LOGS[1]!)
    expect(messages()).toEqual(['could not read the trace again to copy it', 'could not read the trace of the log to copy it'])
  })
})
