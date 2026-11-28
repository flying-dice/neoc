// A fake host page and backend for the stories and tests: recorded traces held in memory, read as the host answers them.
// Nothing here reaches a host, the network or a clock.
import type { ProbeRpc, TelemetryReader } from './telemetry.svelte'
import type { Found, Log, Span, TraceSummary } from './telemetry'

/** 09:41:07 on a day in June: every recorded time is after it. */
export const T0 = new Date(2026, 5, 9, 9, 41, 7).getTime()

const span = (traceId: string, spanId: string, name: string, source: Span['source'], start: number, end: number, extra: Partial<Span> = {}): Span => ({
  traceId,
  spanId,
  name,
  source,
  app: 'services',
  startMs: T0 + start,
  endMs: T0 + end,
  status: 'ok',
  attributes: {},
  ...extra
})

const log = (traceId: string, spanId: string, at: number, severity: Log['severity'], source: Log['source'], body: string, attributes: Record<string, unknown> = {}): Log => ({
  timeMs: T0 + at,
  severity,
  source,
  body,
  app: 'services',
  traceId,
  spanId,
  attributes
})

/** The Services window that loaded and then failed to draw (NEOC-520): the call answered, the render threw. */
export const RENDER_TRACE = 'a1b2c3d4e5f60718293a4b5c6d7e8f90'
/** A call whose handler failed. */
export const FAILED_TRACE = 'f0e1d2c3b4a5968778695a4b3c2d1e0f'

export const SPANS: readonly Span[] = [
  span(RENDER_TRACE, '1111111111111111', 'rpc overview', 'browser', 0, 42, { attributes: { 'rpc.method': 'overview' }, status: 'ok' }),
  span(RENDER_TRACE, '2222222222222222', 'app.call', 'host', 3, 39, { parentSpanId: '1111111111111111', attributes: { method: 'overview' } }),
  span(RENDER_TRACE, '3333333333333333', 'process.run', 'backend', 5, 31, { parentSpanId: '2222222222222222', status: 'unset' }),
  span(RENDER_TRACE, '4444444444444444', 'log.info', 'backend', 33, 34, { parentSpanId: '2222222222222222', status: 'unset' }),
  span(FAILED_TRACE, '5555555555555555', 'rpc restart', 'browser', 5000, 5011, { status: 'error', attributes: { 'rpc.method': 'restart', 'error.code': -32010 } }),
  span(FAILED_TRACE, '6666666666666666', 'app.call', 'host', 5002, 5009, { parentSpanId: '5555555555555555', status: 'error', attributes: { method: 'restart', error: 'nginx.service: access denied' } })
]

export const LOGS: readonly Log[] = [
  log(RENDER_TRACE, '4444444444444444', 33.4, 'info', 'backend', 'read the overview', { units: 212, failed: 1 }),
  log(RENDER_TRACE, '1111111111111111', 44, 'error', 'browser', 'each_key_duplicate: Keyed each block has duplicate key `nginx.service` at indexes 0 and 1', {
    'exception.type': 'Error',
    'vos.correlated': 'last-call'
  }),
  log(FAILED_TRACE, '6666666666666666', 5008, 'error', 'backend', 'systemctl refused the restart', { unit: 'nginx.service' }),
  log(FAILED_TRACE, '6666666666666666', 5008.5, 'error', 'backend', 'app code failed: services/main.luau:40: nginx.service: access denied', {
    error: 'services/main.luau:40: nginx.service: access denied'
  })
]

function summaries(spans: readonly Span[], logs: readonly Log[]): TraceSummary[] {
  const ids = [...new Set(spans.map((s) => s.traceId))]
  return ids
    .map((traceId) => {
      const own = spans.filter((s) => s.traceId === traceId)
      const root = own.find((s) => !s.parentSpanId) ?? own[0]!
      const start = Math.min(...own.map((s) => s.startMs))
      const end = Math.max(...own.map((s) => s.endMs))
      const errors = own.filter((s) => s.status === 'error').length + logs.filter((l) => l.traceId === traceId && l.severity === 'error').length
      return { traceId, name: root.name, app: root.app, startMs: start, durationMs: end - start, spans: own.length, errors }
    })
    .sort((a, b) => b.startMs - a.startMs)
}

const RANK: Record<Log['severity'], number> = { trace: 0, debug: 1, info: 2, warn: 3, error: 4 }

export interface FakeOptions {
  spans?: readonly Span[]
  logs?: readonly Log[]
  /** Every read fails with this error (a host page that does not offer telemetry is code -32601). */
  failWith?: Error & { code?: number }
  /** Reads never answer: a window still loading. */
  hang?: boolean
}

export interface FakeBackend {
  reader: TelemetryReader
  rpc: ProbeRpc
  /** The probes sent, in order: whether each was asked to fail. */
  probes: boolean[]
}

export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  const spans = options.spans ?? SPANS
  const logs = options.logs ?? LOGS
  const probes: boolean[] = []
  const read = <T>(answer: () => T): Promise<T> => {
    if (options.hang) return new Promise<T>(() => {})
    if (options.failWith) return Promise.reject(options.failWith)
    return Promise.resolve(answer())
  }
  return {
    probes,
    reader: {
      traces: () => read(() => summaries(spans, logs)),
      query: (q) =>
        read(
          (): Found => ({
            spans: spans.filter((s) => !q.traceId || s.traceId === q.traceId),
            logs: logs.filter((l) => (!q.traceId || l.traceId === q.traceId) && (!q.severity || RANK[l.severity] >= RANK[q.severity]))
          })
        )
    },
    rpc: {
      probe: (params) => {
        probes.push(params?.fail === true)
        if (params?.fail) return Promise.reject(Object.assign(new Error('the probe failed on purpose'), { code: -32010 }))
        return Promise.resolve({ units: 3 })
      }
    }
  }
}

/** An error as the bridge rejects with it. */
export const rpcError = (code: number, message: string): Error & { code: number } => Object.assign(new Error(message), { code })
