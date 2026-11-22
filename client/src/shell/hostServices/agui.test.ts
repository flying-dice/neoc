import { describe, expect, test } from 'bun:test'
import { HostError } from '../channel'
import { ServiceRegistry, type ServiceContext } from '../hostServices'
import { MAX_BUFFERED_BYTES, MAX_RUNS_PER_FRAME, registerAgui, type FetchLike } from './agui'

interface Sent {
  url: string
  body: unknown
  signal: AbortSignal | undefined
}

/** A response body the test writes SSE frames into, counting how often the reader asked for more. */
function sseBody(): {
  body: ReadableStream<Uint8Array>
  send(event: object): void
  close(): void
  fail(): void
  pulls: () => number
} {
  let controller!: ReadableStreamDefaultController<Uint8Array>
  let pulls = 0
  const body = new ReadableStream<Uint8Array>(
    {
      start(c) {
        controller = c
      },
      pull() {
        pulls++
      }
    },
    { highWaterMark: 0 }
  )
  const encoder = new TextEncoder()
  return {
    body,
    send: (event) => controller.enqueue(encoder.encode(`data: ${JSON.stringify(event)}\n\n`)),
    close: () => controller.close(),
    fail: () => {
      try {
        controller.error(new Error('socket hang up'))
      } catch {
        // already closed
      }
    },
    pulls: () => pulls
  }
}

interface Rig {
  registry: ServiceRegistry
  sent: Sent[]
  context: ServiceContext
  page: AbortController
  call(method: string, params: unknown, context?: ServiceContext): Promise<unknown>
  /** The body the next `POST /agui` answers with. */
  streams: ReturnType<typeof sseBody>[]
}

function rig(
  appId = 'agents',
  answer?: (url: string, body: unknown) => Response | undefined,
  pollIdleMs = 25_000
): Rig {
  const sent: Sent[] = []
  const streams: ReturnType<typeof sseBody>[] = []
  const fetch: FetchLike = async (url, init) => {
    const body = typeof init?.body === 'string' ? (JSON.parse(init.body) as unknown) : undefined
    sent.push({ url, body, signal: init?.signal ?? undefined })
    const custom = answer?.(url, body)
    if (custom) return custom
    if (url === '/agui') {
      const stream = sseBody()
      streams.push(stream)
      init?.signal?.addEventListener('abort', () => stream.fail())
      return new Response(stream.body, { headers: { 'content-type': 'text/event-stream' } })
    }
    if (url === '/agui/threads') {
      const b = body as { threadId: string; agentId: string }
      return Response.json({
        threadId: b.threadId,
        agentId: b.agentId,
        running: false,
        messages: [],
        interrupts: []
      })
    }
    if (url.endsWith('/cancel')) return new Response(null, { status: 204 })
    return Response.json({ session: {} })
  }
  const registry = registerAgui(new ServiceRegistry(), { fetch, pollIdleMs })
  const page = new AbortController()
  const context: ServiceContext = { appId, signal: page.signal }
  return {
    registry,
    sent,
    streams,
    context,
    page,
    call: async (method, params, ctx = context) => {
      const service = registry.get(method)
      if (!service) throw new Error(`no ${method}`)
      return service(params, ctx)
    }
  }
}

const errorOf = async (p: Promise<unknown>): Promise<HostError> => {
  try {
    await p
  } catch (error) {
    if (error instanceof HostError) return error
    throw error
  }
  throw new Error('it did not fail')
}

const tick = (ms = 0): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms))

const startRun = async (
  r: Rig,
  threadId = 'abc',
  runId = 'r1'
): Promise<ReturnType<typeof sseBody>> => {
  await r.call('vos/agui/run', {
    threadId,
    runId,
    agentId: 'scripted',
    messages: [{ id: 'm1', role: 'user', content: 'hello' }]
  })
  return r.streams[r.streams.length - 1]
}

interface Batch {
  events: { type: string; threadId?: string; delta?: string; message?: string }[]
  last: number
  done: boolean
}
const pull = (r: Rig, after: number, threadId = 'abc', runId = 'r1'): Promise<Batch> =>
  r.call('vos/agui/events', { threadId, runId, after }) as Promise<Batch>

describe('whose thread', () => {
  test("the host is sent the frame's app before the page's id, and the page gets its own id back", async () => {
    const r = rig('agents')
    const snapshot = (await r.call('vos/agui/thread', {
      threadId: 'abc',
      agentId: 'scripted'
    })) as {
      threadId: string
    }
    expect(r.sent[0]).toMatchObject({
      url: '/agui/threads',
      body: { threadId: 'agents:abc', agentId: 'scripted' }
    })
    expect(snapshot.threadId).toBe('abc')
  })

  test('the app comes from the frame: two frames with the same id reach two different chats', async () => {
    const r = rig('agents')
    await r.call('vos/agui/thread', { threadId: 'abc', agentId: 'scripted' })
    await r.call(
      'vos/agui/thread',
      { threadId: 'abc', agentId: 'scripted' },
      { appId: 'notes', signal: r.page.signal }
    )
    expect(r.sent.map((s) => (s.body as { threadId: string }).threadId)).toEqual([
      'agents:abc',
      'notes:abc'
    ])
  })

  test.each([
    ['another app named in the id', 'notes:abc'],
    ['a path', '../abc'],
    ['an empty id', ''],
    ['an id over 64 characters', 'a'.repeat(65)],
    ['a number', 42],
    ['an encoded separator', 'a%3Ab']
  ])('a page cannot name %s', async (_why, threadId) => {
    const r = rig()
    for (const [method, params] of [
      ['vos/agui/thread', { threadId, agentId: 'scripted' }],
      ['vos/agui/run', { threadId, runId: 'r1', agentId: 'scripted', messages: [{}] }],
      ['vos/agui/events', { threadId, runId: 'r1', after: 0 }],
      ['vos/agui/cancel', { threadId }],
      ['vos/agui/detach', { threadId }],
      ['vos/agui/settings', { threadId, change: { mode: 'ask' } }]
    ] as const) {
      const error = await errorOf(r.call(method, params))
      expect(error.code).toBe(-32602)
      expect(error.message).toContain(`${method}: params.threadId`)
    }
    expect(r.sent).toEqual([])
  })

  test('a key the service does not know is refused, not passed on', async () => {
    const r = rig()
    const error = await errorOf(
      r.call('vos/agui/thread', { threadId: 'abc', agentId: 'scripted', app: 'notes' })
    )
    expect(error.code).toBe(-32602)
    expect(r.sent).toEqual([])
  })

  test('a run with no message and no answer is refused', async () => {
    const error = await errorOf(
      rig().call('vos/agui/run', { threadId: 'abc', runId: 'r1', agentId: 'scripted' })
    )
    expect(error.message).toBe('vos/agui/run needs messages or resume')
  })

  test('a message over the limit is refused before anything is sent', async () => {
    const r = rig()
    const error = await errorOf(
      r.call('vos/agui/run', {
        threadId: 'abc',
        runId: 'r1',
        agentId: 'scripted',
        messages: [{ id: 'm', role: 'user', content: 'x'.repeat(1_000_001) }]
      })
    )
    expect(error.message).toBe('A message may be at most 1,000,000 bytes')
    expect(r.sent).toEqual([])
  })
})

describe('a run streams by pull', () => {
  test('the run is sent as AG-UI input under the scoped thread, and its events come back in order with the page id', async () => {
    const r = rig()
    const stream = await startRun(r)
    expect(r.sent[0]).toMatchObject({
      url: '/agui',
      body: {
        threadId: 'agents:abc',
        runId: 'r1',
        forwardedProps: { agentId: 'scripted' },
        messages: [{ id: 'm1' }]
      }
    })
    stream.send({ type: 'RUN_STARTED', threadId: 'agents:abc', runId: 'r1' })
    stream.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'a', delta: 'Hi' })
    await tick()
    const first = await pull(r, 0)
    expect(first.events.map((e) => e.type)).toEqual(['RUN_STARTED', 'TEXT_MESSAGE_CONTENT'])
    expect(first.events[0].threadId).toBe('abc')
    expect(first).toMatchObject({ last: 2, done: false })
    stream.send({ type: 'RUN_FINISHED', threadId: 'agents:abc', runId: 'r1' })
    stream.close()
    await tick()
    const second = await pull(r, first.last)
    expect(second.events.map((e) => e.type)).toEqual(['RUN_FINISHED'])
    expect(second).toMatchObject({ last: 3, done: true })
  })

  test('a pull whose answer was lost is asked again from the same place: nothing skipped, nothing repeated', async () => {
    const r = rig()
    const stream = await startRun(r)
    for (const delta of ['a', 'b', 'c'])
      stream.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'm', delta })
    await tick()
    const lost = await pull(r, 0)
    expect(lost.last).toBe(3)
    const again = await pull(r, 0) // the page never saw `lost`
    expect(again.events.map((e) => e.delta)).toEqual(['a', 'b', 'c'])
    stream.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'm', delta: 'd' })
    stream.close()
    await tick()
    const next = await pull(r, again.last)
    expect(next.events.map((e) => e.delta)).toEqual(['d'])
    // the last answer too: a finished run is kept until its place is needed
    expect(await pull(r, 3)).toEqual(next)
    expect(await pull(r, 4)).toEqual({ events: [], last: 4, done: true })
  })

  test('a pull for an event that has not happened is refused', async () => {
    const r = rig()
    await startRun(r)
    expect((await errorOf(pull(r, 5))).message).toBe('There is no event 5 yet')
  })

  test('a pull waits for the next event rather than answering empty', async () => {
    const r = rig()
    const stream = await startRun(r)
    const waiting = pull(r, 0)
    await tick(5)
    stream.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'm', delta: 'late' })
    expect((await waiting).events.map((e) => e.delta)).toEqual(['late'])
  })

  test('a run idle past any time limit is pulled with empty answers that say it is not done, then delivers', async () => {
    const r = rig('agents', undefined, 20)
    const stream = await startRun(r)
    const started = Date.now()
    let empties = 0
    let batch = await pull(r, 0)
    while (batch.events.length === 0 && Date.now() - started < 120) {
      expect(batch).toEqual({ events: [], last: 0, done: false })
      empties++
      batch = await pull(r, batch.last)
    }
    expect(empties).toBeGreaterThanOrEqual(3)
    const late = pull(r, batch.last)
    stream.send({ type: 'RUN_FINISHED', threadId: 'agents:abc', runId: 'r1' })
    stream.close()
    expect((await late).events.map((e) => e.type)).toEqual(['RUN_FINISHED'])
  })

  test('only one pull of a run may wait at a time', async () => {
    const r = rig()
    await startRun(r)
    const waiting = pull(r, 0)
    expect((await errorOf(pull(r, 0))).message).toBe('A pull of this run is already waiting')
    await r.call('vos/agui/detach', { threadId: 'abc' })
    expect((await waiting).done).toBe(true)
  })

  test('a run the page does not pull stops being read at the bound, and reading resumes once it is acknowledged', async () => {
    const r = rig()
    const stream = await startRun(r)
    const big = 'x'.repeat(64 * 1024)
    const chunks = Math.ceil(MAX_BUFFERED_BYTES / big.length) + 4
    for (let i = 0; i < chunks; i++)
      stream.send({ type: 'TEXT_MESSAGE_CONTENT', messageId: 'm', delta: big })
    await tick(10)
    const before = stream.pulls()
    await tick(10)
    expect(stream.pulls()).toBe(before) // paused: nothing more is read while the page holds the bound
    let after = 0
    let delivered = 0
    for (let i = 0; i < 10 && delivered < chunks; i++) {
      const batch = await pull(r, after)
      expect(JSON.stringify(batch).length).toBeLessThan(300 * 1024) // one answer is bounded too
      delivered += batch.events.length
      after = batch.last
      await tick(5)
    }
    expect(delivered).toBe(chunks)
    expect(stream.pulls()).toBeGreaterThan(before)
  })

  test('the host failing before the stream opens is the error the page sees', async () => {
    const r = rig('agents', (url) =>
      url === '/agui'
        ? Response.json({ error: 'the agent could not start: no such command' }, { status: 502 })
        : undefined
    )
    const error = await errorOf(startRun(r))
    expect(error).toMatchObject({
      code: -32000,
      message: 'the agent could not start: no such command',
      data: { status: 502 }
    })
    // and the chat is free again
    expect(await errorOf(pull(r, 0))).toMatchObject({ code: -32602 })
  })

  test('vos going away mid-run ends the run with an error event', async () => {
    const r = rig()
    const stream = await startRun(r)
    stream.send({ type: 'RUN_STARTED', threadId: 'agents:abc', runId: 'r1' })
    stream.fail()
    await tick()
    const batch = await pull(r, 0)
    expect(batch.events.map((e) => e.type)).toEqual(['RUN_STARTED', 'RUN_ERROR'])
    expect(batch.events[1].message).toBe('The connection to vos dropped: socket hang up')
    expect(batch.done).toBe(true)
  })

  test('vos unreachable is an error the page sees', async () => {
    const r = rig('agents', (url) => {
      if (url === '/agui/threads') throw new TypeError('Load failed')
      return undefined
    })
    const error = await errorOf(r.call('vos/agui/thread', { threadId: 'abc', agentId: 'scripted' }))
    expect(error.message).toBe('Could not reach vos: Load failed')
  })
})

describe('limits per frame', () => {
  test('a second run of a chat that is answering is refused', async () => {
    const r = rig()
    await startRun(r)
    const error = await errorOf(startRun(r, 'abc', 'r2'))
    expect(error).toMatchObject({
      code: -32000,
      message: 'This chat is already answering: stop it first'
    })
  })

  test(`the ${MAX_RUNS_PER_FRAME + 1}th chat answering at once is refused with what to do, and a finished one gives its place up`, async () => {
    const r = rig()
    const streams = []
    for (let i = 0; i < MAX_RUNS_PER_FRAME; i++) streams.push(await startRun(r, `t${i}`))
    const error = await errorOf(startRun(r, 'extra'))
    expect(error).toMatchObject({
      code: -32000,
      message: '4 chats are already answering in this window: stop one, or wait for one to finish'
    })
    streams[0].close()
    await tick()
    await startRun(r, 'extra')
    // another frame has places of its own
    const other = rig()
    for (let i = 0; i < MAX_RUNS_PER_FRAME; i++) await startRun(other, `t${i}`)
  })
})

describe('teardown', () => {
  test("the page's document going aborts its streams, ends a waiting pull, and frees the chat", async () => {
    const r = rig()
    await startRun(r)
    const waiting = pull(r, 0)
    r.page.abort()
    expect(r.sent[0].signal?.aborted).toBe(true)
    expect((await waiting).done).toBe(true)
  })

  test('detach drops the stream (the host sees the client go) and the chat can run again', async () => {
    const r = rig()
    await startRun(r)
    await r.call('vos/agui/detach', { threadId: 'abc' })
    expect(r.sent[0].signal?.aborted).toBe(true)
    await startRun(r, 'abc', 'r2')
  })

  test('cancel drops the stream and asks the host to cancel the turn', async () => {
    const r = rig()
    await startRun(r)
    await r.call('vos/agui/cancel', { threadId: 'abc' })
    expect(r.sent[0].signal?.aborted).toBe(true)
    expect(r.sent[1].url).toBe('/agui/threads/agents%3Aabc/cancel')
  })

  test('a run started by a document that is already gone is refused', async () => {
    const r = rig()
    r.page.abort()
    expect((await errorOf(startRun(r))).message).toBe('The page has gone')
  })
})

describe('settings', () => {
  test('a mode or a setting is sent to the scoped thread, and a wrong shape is refused', async () => {
    const r = rig()
    await r.call('vos/agui/settings', { threadId: 'abc', change: { mode: 'plan' } })
    await r.call('vos/agui/settings', {
      threadId: 'abc',
      change: { config: { id: 'model', value: 'fast' } }
    })
    expect(r.sent.map((s) => [s.url, s.body])).toEqual([
      ['/agui/threads/agents%3Aabc/settings', { mode: 'plan' }],
      ['/agui/threads/agents%3Aabc/settings', { config: { id: 'model', value: 'fast' } }]
    ])
    const error = await errorOf(
      r.call('vos/agui/settings', { threadId: 'abc', change: { mode: 'a', extra: 1 } })
    )
    expect(error.code).toBe(-32602)
  })
})
