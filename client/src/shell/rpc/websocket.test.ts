/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { FakeTimers } from './fakeTimers'
import {
  backoffDelay,
  createWebSocketTransport,
  defaultRpcUrl,
  type Backoff,
  type SocketLike
} from './websocket'
import type { TransportStatus } from './transport'

class FakeSocket implements SocketLike {
  static all: FakeSocket[] = []
  readyState = 0
  sent: string[] = []
  onopen: ((ev: unknown) => void) | null = null
  onclose: ((ev: unknown) => void) | null = null
  onerror: ((ev: unknown) => void) | null = null
  onmessage: ((ev: { data: unknown }) => void) | null = null
  constructor(readonly url: string) {
    FakeSocket.all.push(this)
  }
  send(data: string): void {
    this.sent.push(data)
  }
  close(): void {
    this.readyState = 3
    this.onclose?.({})
  }
  open(): void {
    this.readyState = 1
    this.onopen?.({})
  }
  fail(): void {
    this.readyState = 3
    this.onerror?.({})
    this.onclose?.({})
  }
}

const backoff: Backoff = { baseMs: 100, maxMs: 1000, factor: 2, jitter: 0 }

function setup() {
  FakeSocket.all = []
  const timers = new FakeTimers()
  const transport = createWebSocketTransport({
    url: 'ws://host/rpc',
    backoff,
    random: () => 0,
    createSocket: (url) => new FakeSocket(url),
    setTimer: (fn, ms) => timers.setTimeout(fn, ms) as ReturnType<typeof setTimeout>,
    clearTimer: (t) => timers.clearTimeout(t)
  })
  const statuses: TransportStatus[] = []
  transport.onStatus((s) => statuses.push(s))
  return { transport, timers, statuses, sockets: FakeSocket.all }
}

describe('backoff', () => {
  test('doubles from the base and caps at the max', () => {
    expect([0, 1, 2, 3, 4, 5].map((n) => backoffDelay(n, backoff, () => 0))).toEqual([
      100, 200, 400, 800, 1000, 1000
    ])
  })
  test('jitter only ever shortens the delay, within its fraction', () => {
    const j: Backoff = { ...backoff, jitter: 0.5 }
    expect(backoffDelay(2, j, () => 0)).toBe(400)
    expect(backoffDelay(2, j, () => 1)).toBe(200)
  })
  test('the rpc url follows the page protocol', () => {
    expect(defaultRpcUrl({ protocol: 'https:', host: 'a.b:7000' })).toBe('wss://a.b:7000/rpc')
    expect(defaultRpcUrl({ protocol: 'http:', host: 'localhost:5173' })).toBe(
      'ws://localhost:5173/rpc'
    )
  })
})

describe('reconnecting websocket transport', () => {
  test('connects, reports open, and delivers frames both ways', () => {
    const { transport, sockets, statuses } = setup()
    const received: string[] = []
    transport.onMessage((m) => received.push(m))
    transport.connect()
    expect(statuses).toEqual(['connecting'])
    sockets[0].open()
    expect(transport.status).toBe('open')
    transport.send('hello')
    sockets[0].onmessage?.({ data: 'world' })
    expect(sockets[0].sent).toEqual(['hello'])
    expect(received).toEqual(['world'])
  })

  test('sending while not open throws', () => {
    const { transport } = setup()
    expect(() => transport.send('x')).toThrow('Not connected')
  })

  test('a dropped socket reconnects with growing delays, and a successful open resets the backoff', async () => {
    const { transport, timers, sockets } = setup()
    transport.connect()
    sockets[0].open()
    sockets[0].close() // network drop
    expect(transport.status).toBe('closed')
    await timers.advance(99)
    expect(sockets).toHaveLength(1)
    await timers.advance(1)
    expect(sockets).toHaveLength(2) // first retry after 100 ms
    sockets[1].fail()
    await timers.advance(199)
    expect(sockets).toHaveLength(2)
    await timers.advance(1)
    expect(sockets).toHaveLength(3) // second retry after 200 ms
    sockets[2].open()
    sockets[2].close()
    await timers.advance(100)
    expect(sockets).toHaveLength(4) // the backoff started over from the base
  })

  test('close() stops for good: no reconnect, and a late close event from the old socket is ignored', async () => {
    const { transport, timers, sockets } = setup()
    transport.connect()
    sockets[0].open()
    transport.close()
    await timers.advance(10_000)
    expect(sockets).toHaveLength(1)
    expect(transport.status).toBe('closed')
  })

  test('retryNow skips the wait', async () => {
    const { transport, timers, sockets } = setup()
    transport.connect()
    sockets[0].fail()
    expect(sockets).toHaveLength(1)
    transport.retryNow()
    expect(sockets).toHaveLength(2)
    await timers.advance(1000)
    expect(sockets).toHaveLength(2) // the pending timer was cancelled
  })

  test('connect is idempotent', () => {
    const { transport, sockets } = setup()
    transport.connect()
    transport.connect()
    expect(sockets).toHaveLength(1)
  })
})
