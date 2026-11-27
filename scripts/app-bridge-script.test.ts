import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * `crates/vos-http/src/apps/bridge.js` run against a stand-in `window`: what an app's call posts to the host page. It is the
 * script every app's page loads, so this is the one place a framework's objects meet `postMessage`.
 */
const source = readFileSync(join(import.meta.dir, '../crates/vos-http/src/apps/bridge.js'), 'utf8')

interface Tracing {
  span(name: string, fields?: unknown): { in_scope<T>(body: () => T): T; record(key: string, value: unknown): void }
  event(level: string, message: unknown, fields?: unknown): void
}

interface Loaded {
  posted: Record<string, unknown>[]
  /** The page's two globals: `RPC` (its calls) and `vos` (the rest). */
  RPC: { send_request(app: string, method: string, params?: unknown): Promise<unknown> }
  vos: { tracing: Tracing & { query(params: unknown): Promise<unknown> } }
  /** The host page answers a call. */
  answer(id: unknown, result?: unknown): void
  /** What the page recorded, as the host page is sent it (`vos/telemetry`, sent when the page goes). */
  recorded(): { spans: Record<string, unknown>[]; logs: Record<string, unknown>[] }
}

function load(): Loaded {
  const posted: Record<string, unknown>[] = []
  const html = { dataset: {} as Record<string, string>, style: {} as Record<string, string> }
  const listeners: Record<string, ((e: unknown) => void)[]> = {}
  const host = {
    // like the real thing, a message that cannot be structured-cloned throws
    postMessage: (message: Record<string, unknown>) => {
      structuredClone(message)
      posted.push(message)
    }
  }
  const win: Record<string, unknown> = {
    parent: host,
    addEventListener: (name: string, fn: (e: unknown) => void) => (listeners[name] ??= []).push(fn),
    matchMedia: () => ({ matches: false }),
    location: { search: '' }
  }
  const doc = {
    documentElement: html,
    readyState: 'loading',
    addEventListener: () => undefined,
    getElementById: () => null,
    body: null
  }
  new Function('window', 'parent', 'document', 'location', 'URLSearchParams', source).call(
    win,
    win,
    host,
    doc,
    win.location,
    URLSearchParams
  )
  const fire = (name: string, e: unknown): void => (listeners[name] ?? []).forEach((fn) => fn(e))
  return {
    posted,
    RPC: win.RPC as never,
    vos: win.vos as never,
    answer: (id, result = {}) => fire('message', { source: host, data: { jsonrpc: '2.0', id, result } }),
    recorded: () => {
      fire('pagehide', {})
      const batches = posted.filter((m) => m.method === 'vos/telemetry').map((m) => m.params as { spans: []; logs: [] })
      return { spans: batches.flatMap((b) => b.spans), logs: batches.flatMap((b) => b.logs) }
    }
  }
}

/** What a request of the bus for `method` posted: `vos/request`, with the app, the method and its params. */
const sentFor = (posted: Record<string, unknown>[], method: string): Record<string, unknown> | undefined =>
  posted.find((m) => m.method === 'vos/request' && (m.params as { method?: string }).method === method)

const traceOf = (call: Record<string, unknown> | undefined): string[] =>
  String((call?._meta as { traceparent?: string } | undefined)?.traceparent).split('-')

describe('the app side of the bridge posts plain JSON', () => {
  test('a reactive proxy as the arguments is posted as the plain object it stands for', async () => {
    const { posted, RPC } = load()
    // what a Svelte 5 `$state` object is to `postMessage`: a Proxy, which structuredClone refuses
    const reactive = new Proxy({ title: 'Buy milk', tags: ['a'] }, {})
    expect(() => structuredClone(reactive)).toThrow()
    void RPC.send_request('notes', 'add', reactive)
    expect(sentFor(posted, 'add')?.params).toEqual({ app: 'notes', method: 'add', params: { title: 'Buy milk', tags: ['a'] } })
  })

  test('no arguments are an empty object, and arguments that are not JSON reject with the method named', async () => {
    const { posted, RPC } = load()
    void RPC.send_request('notes', 'list')
    expect((sentFor(posted, 'list')?.params as { params: unknown }).params).toEqual({})
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    await expect(RPC.send_request('notes', 'add', cyclic)).rejects.toThrow(/The arguments of "add" are not JSON/)
    expect(sentFor(posted, 'add')).toBeUndefined()
  })
})

describe('the names the page has', () => {
  test('the page has RPC and vos.host and vos.tracing, and nothing of the deprecated underlayer', () => {
    const { vos } = load()
    expect(Object.keys(vos).sort()).toEqual(['host', 'tracing'])
    expect(Object.keys(vos.tracing)).toContain('query')
  })

  test('a plain method name is no call of the page’s own backend any more: nothing posts one', () => {
    const { posted, RPC } = load()
    void RPC.send_request('notes', 'press')
    expect(posted.filter((m) => m.method === 'press')).toEqual([])
    expect(sentFor(posted, 'press')).toBeDefined()
  })

  test('vos.tracing.query asks the host for what was recorded, untraced', () => {
    const { posted, vos } = load()
    void vos.tracing.query({ kind: 'traces' })
    const sent = posted.find((m) => m.method === 'vos/telemetry/query')
    expect(sent?.params).toEqual({ kind: 'traces' })
    expect(sent?._meta).toBeUndefined()
  })
})

describe('tracing: the span the page is in', () => {
  test('a call made in a span is its child, and its traceparent carries the call’s span', async () => {
    const { posted, RPC, vos, answer, recorded } = load()
    const done = vos.tracing.span('load', { scope: 'system' }).in_scope(() => RPC.send_request('notes', 'overview'))
    const call = sentFor(posted, 'overview')
    answer(call?.id)
    await done
    const { spans } = recorded()
    const load_ = spans.find((s) => s.name === 'load')!
    const rpc = spans.find((s) => s.name === 'rpc overview')!
    const [, traceId, spanId] = traceOf(call)
    expect([rpc.traceId, rpc.spanId, rpc.parentSpanId]).toEqual([traceId, spanId, load_.spanId])
    expect([load_.traceId, load_.status, (load_.attributes as { scope: string }).scope]).toEqual([traceId, 'ok', 'system'])
  })

  test('after an await, a call is still the child of the one scope pending; with two pending it is neither’s', async () => {
    const { posted, RPC, vos, answer, recorded } = load()
    const one = vos.tracing.span('one').in_scope(async () => {
      await Promise.resolve()
      return RPC.send_request('notes', 'later')
    })
    await Promise.resolve()
    await Promise.resolve()
    answer(sentFor(posted, 'later')?.id)
    await one
    let gate!: () => void
    const held = new Promise<void>((resolve) => (gate = resolve))
    const a = vos.tracing.span('a').in_scope(() => held)
    const b = vos.tracing.span('b').in_scope(() => held)
    void RPC.send_request('notes', 'ambiguous')
    gate()
    await Promise.all([a, b])
    const { spans } = recorded()
    const parentOf = (name: string): unknown => spans.find((s) => s.name === name)?.parentSpanId
    expect(parentOf('rpc later')).toBe(spans.find((s) => s.name === 'one')?.spanId)
    expect(parentOf('rpc ambiguous')).toBeUndefined()
  })

  test('an event is in the span it was written in; a throw fails the span and is thrown again', () => {
    const { vos, recorded } = load()
    expect(() =>
      vos.tracing.span('draw').in_scope(() => {
        vos.tracing.event('info', 'drawing', { count: 2 })
        throw new Error('each_key_duplicate')
      })
    ).toThrow('each_key_duplicate')
    const { spans, logs } = recorded()
    const draw = spans.find((s) => s.name === 'draw')!
    expect([draw.status, (draw.attributes as Record<string, unknown>)['error.message']]).toEqual(['error', 'each_key_duplicate'])
    expect(logs.find((l) => l.body === 'drawing')?.spanId).toBe(draw.spanId)
  })
})
