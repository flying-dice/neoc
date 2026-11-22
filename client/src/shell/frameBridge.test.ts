import { describe, expect, test } from 'bun:test'
import { HostError } from './channel'
import { MAX_IN_FLIGHT_PER_FRAME, MAX_IN_FLIGHT_TOTAL } from './callGate'
import {
  createFrameBridge,
  eventFor,
  jsonDepth,
  MAX_ARGS_BYTES,
  refuseArgs,
  type FrameBridge,
  type ToFrame
} from './frameBridge'

const FRAME = { name: 'the frame window' }

interface Rig {
  bridge: FrameBridge
  posted: ToFrame[]
  calls: { method: string; params: Record<string, unknown> }[]
  events: { ready: number; failed: string[] }
  flush(): Promise<unknown>
}

function rig(call: (method: string, params: Record<string, unknown>) => Promise<unknown>): Rig {
  const posted: ToFrame[] = []
  const calls: { method: string; params: Record<string, unknown> }[] = []
  const events = { ready: 0, failed: [] as string[] }
  const bridge = createFrameBridge({
    appId: 'notes',
    frame: () => FRAME,
    post: (m) => posted.push(m),
    call: (method, params) => {
      calls.push({ method, params })
      return call(method, params)
    },
    onReady: () => events.ready++,
    onFailed: (m) => events.failed.push(m)
  })
  const flush = (): Promise<unknown> => new Promise((r) => setTimeout(r, 0))
  return { bridge, posted, calls, events, flush }
}

describe('the frame bridge', () => {
  test('an rpc becomes an app.call for the frame’s own app and its result goes back to the caller', async () => {
    const { bridge, posted, calls, flush } = rig(async () => ({ items: [1, 2] }))
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 7, method: 'list', args: { limit: 2 } } })
    await flush()
    expect(calls).toEqual([
      { method: 'app.call', params: { app: 'notes', method: 'list', args: { limit: 2 } } }
    ])
    expect(posted).toEqual([{ t: 'res', id: 7, result: { items: [1, 2] } }])
  })

  test('an app named in the message is not read: the call is for the frame’s app', async () => {
    const { bridge, calls, flush } = rig(async () => null)
    bridge.receive({
      source: FRAME,
      data: { t: 'rpc', id: 1, method: 'list', args: null, app: 'agents' }
    })
    await flush()
    expect(calls[0]?.params.app).toBe('notes')
  })

  test('a message from another window is ignored', async () => {
    const { bridge, calls, posted, events, flush } = rig(async () => null)
    for (const t of ['rpc', 'ready', 'failed'])
      bridge.receive({ source: { other: true }, data: { t, id: 1, method: 'list', message: 'x' } })
    bridge.receive({ source: null, data: { t: 'rpc', id: 1, method: 'list' } })
    await flush()
    expect(calls).toEqual([])
    expect(posted).toEqual([])
    expect(events).toEqual({ ready: 0, failed: [] })
  })

  test('a JSON-RPC error of the host comes back with its code, message and data', async () => {
    const { bridge, posted, flush } = rig(() =>
      Promise.reject(new HostError(-32602, 'Invalid params: params.title is required', { field: 'title' }))
    )
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 3, method: 'add', args: {} } })
    await flush()
    expect(posted).toEqual([
      {
        t: 'res',
        id: 3,
        error: { code: -32602, message: 'Invalid params: params.title is required', data: { field: 'title' } }
      }
    ])
  })

  test('any other failure is an internal error with its message', async () => {
    const { bridge, posted, flush } = rig(() => Promise.reject(new Error('socket closed')))
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 4, method: 'add', args: {} } })
    await flush()
    expect(posted).toEqual([{ t: 'res', id: 4, error: { code: -32603, message: 'socket closed' } }])
  })

  test('a call that cannot be read is answered, so the client’s promise settles', async () => {
    const { bridge, posted, calls, flush } = rig(async () => null)
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 1, method: '' } })
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 2, method: 'x'.repeat(129) } })
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 3, method: 7 } })
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 4 } })
    await flush()
    expect(calls).toEqual([])
    expect(posted).toEqual([
      { t: 'res', id: 1, error: { code: -32600, message: 'A method name is 1 to 128 characters' } },
      { t: 'res', id: 2, error: { code: -32600, message: 'A method name is 1 to 128 characters' } },
      { t: 'res', id: 3, error: { code: -32600, message: 'That is not a call: it needs a method name' } },
      { t: 'res', id: 4, error: { code: -32600, message: 'That is not a call: it needs a method name' } }
    ])
  })

  test('messages that are not calls at all are dropped', async () => {
    const { bridge, calls, posted, flush } = rig(async () => null)
    for (const data of [
      null,
      'rpc',
      { t: 'rpc' },
      { t: 'rpc', id: 'x', method: 'list' },
      { t: 'rpc', id: -1, method: 'a' },
      { t: 'nope' }
    ])
      bridge.receive({ source: FRAME, data })
    await flush()
    expect(calls).toEqual([])
    expect(posted).toEqual([])
  })

  test('a call whose arguments are too big or too deep is refused before it reaches the socket', async () => {
    const { bridge, posted, calls, flush } = rig(async () => null)
    bridge.receive({
      source: FRAME,
      data: { t: 'rpc', id: 1, method: 'add', args: { title: 'x'.repeat(2 * 1024 * 1024) } }
    })
    let deep: unknown = 1
    for (let i = 0; i < 150; i++) deep = [deep]
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 2, method: 'add', args: deep } })
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 3, method: 'add', args: cyclic } })
    await flush()
    expect(calls, 'nothing reached the host connection').toEqual([])
    expect(posted.map((m) => (m as { error: { code: number; message: string } }).error)).toEqual([
      { code: -32602, message: expect.stringContaining('a call may carry at most 1,000,000') },
      { code: -32602, message: 'The arguments are nested more than 100 levels deep' },
      { code: -32602, message: expect.stringContaining('The arguments cannot be sent') }
    ])
  })

  test('arguments just under the limit go through', async () => {
    const { bridge, calls, flush } = rig(async () => null)
    bridge.receive({
      source: FRAME,
      data: { t: 'rpc', id: 1, method: 'add', args: { title: 'x'.repeat(MAX_ARGS_BYTES - 100) } }
    })
    await flush()
    expect(calls.length).toBe(1)
  })

  test('ready and failed reach the shell', () => {
    const { bridge, events } = rig(async () => null)
    bridge.receive({ source: FRAME, data: { t: 'ready' } })
    bridge.receive({ source: FRAME, data: { t: 'failed', message: 'no mount' } })
    expect(events).toEqual({ ready: 1, failed: ['no mount'] })
  })

  test('a disposed bridge neither acts nor answers', async () => {
    const { bridge, posted, calls, flush } = rig(
      () => new Promise((r) => setTimeout(() => r('late'), 20))
    )
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 1, method: 'list' } })
    await flush()
    bridge.dispose()
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 2, method: 'list' } })
    bridge.event('changed', null)
    await new Promise((r) => setTimeout(r, 40))
    expect(calls.length).toBe(1)
    expect(posted).toEqual([])
  })

  test('hello reaches the shell, from the frame only', () => {
    let hellos = 0
    const bridge = createFrameBridge({
      appId: 'notes',
      frame: () => FRAME,
      post: () => {},
      call: async () => null,
      onHello: () => hellos++
    })
    bridge.receive({ source: FRAME, data: { t: 'hello' } })
    bridge.receive({ source: { other: true }, data: { t: 'hello' } })
    expect(hellos).toBe(1)
  })

  test('a burst of calls completes through the gate and never has more than the gate allows in flight', async () => {
    let now = 0
    let peak = 0
    const { bridge, posted } = rig(async () => {
      now += 1
      peak = Math.max(peak, now)
      await new Promise((r) => setTimeout(r, 1))
      now -= 1
      return 'ok'
    })
    for (let id = 0; id < 200; id++)
      bridge.receive({ source: FRAME, data: { t: 'rpc', id, method: 'echo' } })
    for (let i = 0; i < 200 && posted.length < 200; i++) await new Promise((r) => setTimeout(r, 5))
    expect(posted.length).toBe(200)
    expect(posted.every((m) => 'result' in m)).toBe(true)
    expect(peak).toBeLessThanOrEqual(MAX_IN_FLIGHT_PER_FRAME)
  })

  test('two frames and their calls together stay within the total, so the shell’s own calls keep their slots', async () => {
    let now = 0
    let peak = 0
    const slow = async (): Promise<string> => {
      now += 1
      peak = Math.max(peak, now)
      await new Promise((r) => setTimeout(r, 2))
      now -= 1
      return 'ok'
    }
    const a = rig(slow)
    const b = rig(slow)
    for (let id = 0; id < 100; id++) {
      a.bridge.receive({ source: FRAME, data: { t: 'rpc', id, method: 'echo' } })
      b.bridge.receive({ source: FRAME, data: { t: 'rpc', id, method: 'echo' } })
    }
    for (let i = 0; i < 400 && a.posted.length + b.posted.length < 200; i++)
      await new Promise((r) => setTimeout(r, 5))
    expect(a.posted.length + b.posted.length).toBe(200)
    expect(peak).toBeLessThanOrEqual(MAX_IN_FLIGHT_TOTAL)
  })

  test('events go into the frame', () => {
    const { bridge, posted } = rig(async () => null)
    bridge.event('host:theme', 'dark')
    expect(posted).toEqual([{ t: 'event', name: 'host:theme', data: 'dark' }])
  })
})

describe('the arguments of a call', () => {
  test('are measured in bytes, not characters', () => {
    expect(refuseArgs({ t: 'é'.repeat(MAX_ARGS_BYTES / 2 + 10) })).toContain('bytes')
    expect(refuseArgs({ t: 'a'.repeat(1000) })).toBeNull()
    expect(refuseArgs(undefined)).toBeNull()
    expect(refuseArgs(null)).toBeNull()
  })
  test('depth counts open arrays and objects outside strings', () => {
    expect(jsonDepth('[[{"a":[1]}]]')).toBe(4)
    expect(jsonDepth('{"a":"[[[[ \\" ]]]]"}')).toBe(1)
    expect(jsonDepth('1')).toBe(0)
  })
})

describe('eventFor', () => {
  test('is the event of an app.event notification for this app', () => {
    expect(
      eventFor('notes', 'app.event', { app: 'notes', name: 'changed', data: { n: 1 } })
    ).toEqual({
      name: 'changed',
      data: { n: 1 }
    })
  })
  test('is nothing for another app, another notification or a malformed one', () => {
    expect(
      eventFor('notes', 'app.event', { app: 'agents', name: 'changed', data: null })
    ).toBeNull()
    expect(eventFor('notes', 'window.open', { app: 'notes', name: 'x', data: null })).toBeNull()
    expect(eventFor('notes', 'app.event', { app: 'notes' })).toBeNull()
  })
})
