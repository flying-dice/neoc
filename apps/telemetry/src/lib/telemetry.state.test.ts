import { describe, expect, test } from 'bun:test'
import { Telemetry, RENDER_FAILURE } from './telemetry.svelte'
import { FAILED_TRACE, RENDER_TRACE, createFakeBackend, rpcError } from './fakeApi'

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
    const tracer = { span: (name: string, fields?: unknown) => (opened.push([name, fields]), { inScope: <T,>(body: () => T) => body() }) }
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
