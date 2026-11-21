import { describe, expect, test } from 'bun:test'
import { HostError } from './channel'
import { createFrameBridge, eventFor, type FrameBridge, type ToFrame } from './frameBridge'

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

  test('malformed messages are dropped', async () => {
    const { bridge, calls, flush } = rig(async () => null)
    for (const data of [
      null,
      'rpc',
      { t: 'rpc' },
      { t: 'rpc', id: 'x', method: 'list' },
      { t: 'rpc', id: 1, method: '' },
      { t: 'rpc', id: 1, method: 'x'.repeat(129) },
      { t: 'nope' }
    ])
      bridge.receive({ source: FRAME, data })
    await flush()
    expect(calls).toEqual([])
  })

  test('ready and failed reach the shell', () => {
    const { bridge, events } = rig(async () => null)
    bridge.receive({ source: FRAME, data: { t: 'ready' } })
    bridge.receive({ source: FRAME, data: { t: 'failed', message: 'no mount' } })
    expect(events).toEqual({ ready: 1, failed: ['no mount'] })
  })

  test('a disposed bridge neither acts nor answers', async () => {
    const { bridge, posted, calls, flush } = rig(async () => 'late')
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 1, method: 'list' } })
    bridge.dispose()
    bridge.receive({ source: FRAME, data: { t: 'rpc', id: 2, method: 'list' } })
    bridge.event('changed', null)
    await flush()
    expect(calls.length).toBe(1)
    expect(posted).toEqual([])
  })

  test('events go into the frame', () => {
    const { bridge, posted } = rig(async () => null)
    bridge.event('host:theme', 'dark')
    expect(posted).toEqual([{ t: 'event', name: 'host:theme', data: 'dark' }])
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
