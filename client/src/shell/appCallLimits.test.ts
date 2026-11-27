/**
 * What the shell's connection carries when an app's handlers hang: the frame bridge, the gate and the host channel together,
 * against a stand-in host that keeps the real host's rule (`MAX_IN_FLIGHT`, 64 requests in flight on a connection, a permit
 * held until the handler answers; the answer for one over the limit is an error). The channel gives up on a shell request
 * after `timeoutMs`; an app's call must not, or the frame's place is given back while the host still runs the handler.
 */
import { describe, expect, test } from 'bun:test'
import { createHostChannel } from './channel'
import { createCallGate, MAX_IN_FLIGHT_PER_FRAME, MAX_IN_FLIGHT_TOTAL } from './callGate'
import { createHostBridge, type HostBridge, type ToFrame } from './hostBridge'
import { hostContextOf } from './hostContext'
import { baseServices } from './hostServices'

const HOST_LIMIT = 64
const SHELL_TIMEOUT_MS = 20

interface Held {
  id: string
  app: string
  method: string
}

/** A host that holds every `app.call` of method `hang` until released, answers everything else at once, and has 64 permits. */
function standInHost(): {
  transport: Parameters<typeof createHostChannel>[0]
  held: Held[]
  /** The most requests the host ever had running at once, and the most for one app. */
  peak: { total: number; byApp: Map<string, number> }
  refused: string[]
  cancelled: string[]
  release(app?: string): void
  disconnect(): void
} {
  const messages = new Set<(m: string) => void>()
  const statuses = new Set<(s: 'connecting' | 'open' | 'closed') => void>()
  const held: Held[] = []
  const refused: string[] = []
  const cancelled: string[] = []
  const peak = { total: 0, byApp: new Map<string, number>() }
  let running = 0
  const running_by = new Map<string, number>()
  const reply = (frame: object): void =>
    queueMicrotask(() => messages.forEach((l) => l(JSON.stringify(frame))))
  const finish = (h: Held): void => {
    running -= 1
    running_by.set(h.app, (running_by.get(h.app) ?? 1) - 1)
  }
  return {
    held,
    peak,
    refused,
    cancelled,
    transport: {
      status: 'open',
      connect: () => undefined,
      close: () => undefined,
      retryNow: () => undefined,
      onMessage: (l) => (messages.add(l), () => void messages.delete(l)),
      onStatus: (l) => (statuses.add(l), () => void statuses.delete(l)),
      send: (raw) => {
        const msg = JSON.parse(raw) as { method: string; params: { requestId?: string } }
        if (msg.method === 'notifications/cancelled') {
          // the host stops the running request and frees its place; no reply (the specification's rule)
          const i = held.findIndex((h) => h.id === msg.params.requestId)
          if (i >= 0) {
            finish(held[i] as Held)
            held.splice(i, 1)
            cancelled.push(msg.params.requestId as string)
          }
          return
        }
        const req = JSON.parse(raw) as {
          id: string
          method: string
          params: { app?: string; method?: string }
        }
        if (running >= HOST_LIMIT) {
          refused.push(req.method)
          return reply({
            jsonrpc: '2.0',
            id: req.id,
            error: { code: -32000, message: 'too many requests in flight' }
          })
        }
        const app = req.params.app ?? '(shell)'
        running += 1
        running_by.set(app, (running_by.get(app) ?? 0) + 1)
        peak.total = Math.max(peak.total, running)
        peak.byApp.set(app, Math.max(peak.byApp.get(app) ?? 0, running_by.get(app) ?? 0))
        if (req.method === 'app.call' && req.params.method === 'hang') {
          held.push({ id: req.id, app, method: 'hang' })
          return
        }
        finish({ id: req.id, app, method: req.method })
        reply({ jsonrpc: '2.0', id: req.id, result: { ok: true } })
      }
    },
    release(only) {
      for (const h of held.splice(0).filter((x) => {
        if (only === undefined || x.app === only) return true
        held.push(x)
        return false
      })) {
        finish(h)
        reply({ jsonrpc: '2.0', id: h.id, result: 'released' })
      }
    },
    disconnect() {
      // the connection's permits go with the connection: a new one starts with all 64
      running = 0
      running_by.clear()
      held.splice(0)
      for (const l of statuses) l('closed')
    }
  }
}

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms))
const FRAME = { name: 'a frame window' }

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type -- the rig's shape is its own
function frameOf(
  app: string,
  host: ReturnType<typeof standInHost>,
  channel: ReturnType<typeof createHostChannel>,
  gate = sharedGate
) {
  const posted: ToFrame[] = []
  const context = hostContextOf({
    appearance: 'light',
    displayMode: 'inline',
    availableDisplayModes: ['inline']
  })
  const bridge: HostBridge = createHostBridge({
    appId: app,
    frame: () => FRAME,
    post: (m) => posted.push(m),
    call: (method, params, options) => channel.call(method, params, options),
    services: baseServices({
      context: () => context,
      openLink: () => undefined,
      setDisplayMode: (m) => m
    }),
    context: () => context,
    gate
  })
  let id = 0
  return {
    posted,
    bridge,
    // a call of the frame's own app's method, the way a page makes it: on the bus (`vos/request`)
    rpc: (method: string): number => {
      const n = ++id
      bridge.receive({
        source: FRAME,
        data: { jsonrpc: '2.0', id: n, method: 'vos/request', params: { app, method, params: {} } }
      })
      return n
    },
    answered: (n: number): ToFrame | undefined => posted.find((m) => 'id' in m && m.id === n),
    host
  }
}

let sharedGate = createCallGate()

describe('an app whose handlers hang', () => {
  test('the host never has more than its own eight in flight, however long the client has waited, and the shell and other apps carry on', async () => {
    sharedGate = createCallGate()
    const host = standInHost()
    const channel = createHostChannel(host.transport, SHELL_TIMEOUT_MS)
    const hung = frameOf('hung', host, channel)
    const other = frameOf('other', host, channel)

    // far more calls than places, and far longer than the shell's own timeout, so the old behaviour (a place given back when the
    // client stopped waiting) would let the host be filled
    for (let round = 0; round < 12; round++) {
      for (let i = 0; i < 10; i++) hung.rpc('hang')
      await sleep(SHELL_TIMEOUT_MS * 2)
    }
    expect(host.held.length).toBe(MAX_IN_FLIGHT_PER_FRAME)
    expect(host.peak.byApp.get('hung')).toBeLessThanOrEqual(MAX_IN_FLIGHT_PER_FRAME)
    expect(host.peak.total).toBeLessThanOrEqual(MAX_IN_FLIGHT_TOTAL)

    // the shell's own call and another app's call go through at once
    await expect(channel.call('app.list')).resolves.toEqual({ ok: true })
    const n = other.rpc('list')
    await sleep(5)
    expect(other.answered(n)).toEqual({ jsonrpc: '2.0', id: n, result: { ok: true } })
    expect(host.refused).toEqual([])

    // a hung app's call is not lost: it waits, and goes when a handler answers
    host.release('hung')
    await sleep(5)
    expect(host.held.length).toBeGreaterThan(0) // the waiting ones started
    expect(host.peak.byApp.get('hung')).toBeLessThanOrEqual(MAX_IN_FLIGHT_PER_FRAME)
  })

  test('closing a frame ends the calls the host is running for it and frees their places; the calls it had waiting never start', async () => {
    sharedGate = createCallGate()
    const host = standInHost()
    const channel = createHostChannel(host.transport, SHELL_TIMEOUT_MS)
    const first = frameOf('hung', host, channel)
    for (let i = 0; i < 20; i++) first.rpc('hang')
    await sleep(5)
    expect(host.held.length).toBe(MAX_IN_FLIGHT_PER_FRAME)
    first.bridge.dispose() // the window closed: 8 running on the host, 12 waiting in the gate
    await sleep(5)
    // the host was told to stop the eight (nothing of the app is left running), and none of the 12 ever started
    expect(host.cancelled.length).toBe(MAX_IN_FLIGHT_PER_FRAME)
    expect(host.held.length).toBe(0)
    // a window opened again starts with every place free
    const second = frameOf('hung', host, channel)
    for (let i = 0; i < 3; i++) second.rpc('hang')
    await sleep(5)
    expect(host.held.length).toBe(3)
    expect(host.peak.byApp.get('hung')).toBe(MAX_IN_FLIGHT_PER_FRAME)
  })

  test('the connection going frees every place, and the calls are answered with the reason', async () => {
    sharedGate = createCallGate()
    const host = standInHost()
    const channel = createHostChannel(host.transport, SHELL_TIMEOUT_MS)
    const hung = frameOf('hung', host, channel)
    const ids = Array.from({ length: 8 }, () => hung.rpc('hang'))
    await sleep(5)
    host.disconnect()
    await sleep(5)
    for (const n of ids)
      expect(hung.answered(n)).toMatchObject({
        jsonrpc: '2.0',
        error: { message: 'The connection to vos closed' }
      })
    // a place is free again: the next call starts
    hung.rpc('list')
    await sleep(5)
    expect(host.peak.byApp.get('hung')).toBeLessThanOrEqual(MAX_IN_FLIGHT_PER_FRAME)
  })

  test('a shell request still times out: only an app’s call waits for the host', async () => {
    sharedGate = createCallGate()
    const host = standInHost()
    const channel = createHostChannel(host.transport, SHELL_TIMEOUT_MS)
    await expect(channel.call('app.call', { app: 'x', method: 'hang' })).rejects.toThrow(
      /timed out/
    )
  })
})
