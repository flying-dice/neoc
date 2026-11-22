import { describe, expect, test } from 'bun:test'
import { HostError } from './channel'
import {
  createHostBridge,
  eventFor,
  jsonDepth,
  MAX_ARGS_BYTES,
  PROTOCOL_VERSION,
  refuseArgs,
  type HostBridge,
  type ToFrame
} from './hostBridge'
import { MAX_IN_FLIGHT_PER_FRAME } from './callGate'
import { contextChange, hostContextOf, type HostContext } from './hostContext'
import { baseServices, ServiceRegistry } from './hostServices'

const FRAME = { name: 'the frame window' }

const context = (over: Partial<HostContext> = {}): HostContext => ({
  ...hostContextOf({
    appearance: 'light',
    displayMode: 'inline',
    availableDisplayModes: ['inline', 'fullscreen']
  }),
  ...over
})

interface Rig {
  bridge: HostBridge
  posted: ToFrame[]
  calls: { method: string; params: Record<string, unknown> }[]
  events: { ready: number; failed: string[] }
  opened: string[]
  flush(): Promise<unknown>
}

function rig(
  call: (method: string, params: Record<string, unknown>) => Promise<unknown> = async () => null,
  services?: (base: ServiceRegistry) => ServiceRegistry
): Rig {
  const posted: ToFrame[] = []
  const calls: { method: string; params: Record<string, unknown> }[] = []
  const events = { ready: 0, failed: [] as string[] }
  const opened: string[] = []
  let current = context()
  const base = baseServices({
    context: () => current,
    openLink: (url) => opened.push(url.href),
    setDisplayMode: (mode) => {
      current = { ...current, displayMode: mode }
      return mode
    }
  })
  const bridge = createHostBridge({
    appId: 'notes',
    frame: () => FRAME,
    post: (m) => posted.push(m),
    call: (method, params) => {
      calls.push({ method, params })
      return call(method, params)
    },
    services: services ? services(base) : base,
    context: () => current,
    onReady: () => events.ready++,
    onFailed: (m) => events.failed.push(m)
  })
  const flush = (): Promise<unknown> => new Promise((r) => setTimeout(r, 0))
  return { bridge, posted, calls, events, opened, flush }
}

const request = (
  id: number | string,
  method: string,
  params?: unknown
): { jsonrpc: string; id: number | string; method: string; params?: unknown } => ({
  jsonrpc: '2.0',
  id,
  method,
  ...(params === undefined ? {} : { params })
})

describe('the handshake', () => {
  test('ui/initialize is answered with the host’s capabilities, info and context', async () => {
    const { bridge, posted, flush } = rig()
    bridge.receive({
      source: FRAME,
      data: request(1, 'ui/initialize', { protocolVersion: PROTOCOL_VERSION, appCapabilities: {} })
    })
    await flush()
    const reply = posted[0] as { id: number; result: Record<string, unknown> }
    expect(reply.id).toBe(1)
    expect(reply.result.protocolVersion).toBe(PROTOCOL_VERSION)
    expect(reply.result.hostInfo).toEqual({ name: 'vos', version: '1' })
    // the shell arranges windows, so it reports that it draws UI of its own
    expect(reply.result.hostCapabilities).toEqual({
      openLinks: {},
      experimental: { vos: { backend: {}, events: {}, ui: {} } }
    })
    expect(reply.result.hostContext).toMatchObject({
      theme: 'light',
      displayMode: 'inline',
      platform: 'web'
    })
  })

  test('without the app’s capabilities it is an invalid call', async () => {
    const { bridge, posted } = rig()
    bridge.receive({ source: FRAME, data: request(1, 'ui/initialize', { protocolVersion: '1' }) })
    expect(posted).toEqual([
      {
        jsonrpc: '2.0',
        id: 1,
        error: { code: -32602, message: 'ui/initialize needs protocolVersion and appCapabilities' }
      }
    ])
  })

  test('initialized and failed reach the page', () => {
    const { bridge, events } = rig()
    bridge.receive({
      source: FRAME,
      data: { jsonrpc: '2.0', method: 'ui/notifications/initialized', params: {} }
    })
    bridge.receive({
      source: FRAME,
      data: { jsonrpc: '2.0', method: 'vos/failed', params: { message: 'it threw' } }
    })
    bridge.receive({ source: FRAME, data: { jsonrpc: '2.0', method: 'vos/failed' } })
    expect(events.ready).toBe(1)
    expect(events.failed).toEqual(['it threw', 'The app could not start.'])
  })

  test('a ping is answered', () => {
    const { bridge, posted } = rig()
    bridge.receive({ source: FRAME, data: request('p', 'ping') })
    expect(posted).toEqual([{ jsonrpc: '2.0', id: 'p', result: {} }])
  })
})

describe('the app’s own calls', () => {
  test('a method of the app’s backend becomes an app.call for the frame’s own app', async () => {
    const { bridge, posted, calls, flush } = rig(async () => ({ items: [1, 2] }))
    bridge.receive({ source: FRAME, data: request(7, 'list', { limit: 2 }) })
    await flush()
    expect(calls).toEqual([
      { method: 'app.call', params: { app: 'notes', method: 'list', args: { limit: 2 } } }
    ])
    expect(posted).toEqual([{ jsonrpc: '2.0', id: 7, result: { items: [1, 2] } }])
  })

  test('an app named in the message is not read: the call is for the frame’s app', async () => {
    const { bridge, calls, flush } = rig()
    bridge.receive({ source: FRAME, data: { ...request(1, 'list', {}), app: 'agents' } })
    bridge.receive({ source: FRAME, data: request(2, 'list', { app: 'agents' }) })
    await flush()
    expect(calls.map((c) => c.params.app)).toEqual(['notes', 'notes'])
  })

  test('a JSON-RPC error of the host comes back with its code, message and data', async () => {
    const { bridge, posted, flush } = rig(() =>
      Promise.reject(
        new HostError(-32602, 'Invalid params: params.title is required', { field: 'title' })
      )
    )
    bridge.receive({ source: FRAME, data: request(3, 'add', {}) })
    await flush()
    expect(posted).toEqual([
      {
        jsonrpc: '2.0',
        id: 3,
        error: {
          code: -32602,
          message: 'Invalid params: params.title is required',
          data: { field: 'title' }
        }
      }
    ])
  })

  test('any other failure is an internal error with its message', async () => {
    const { bridge, posted, flush } = rig(() => Promise.reject(new Error('socket closed')))
    bridge.receive({ source: FRAME, data: request(4, 'add', {}) })
    await flush()
    expect(posted).toEqual([
      { jsonrpc: '2.0', id: 4, error: { code: -32603, message: 'socket closed' } }
    ])
  })
})

describe('what the host does not offer is refused in words', () => {
  test('a ui/ or vos/ method no service registered answers "does not offer"', async () => {
    const { bridge, posted, calls } = rig()
    for (const name of [
      'ui/message',
      'ui/update-model-context',
      'vos/toast',
      'tools/call',
      'resources/read'
    ]) {
      bridge.receive({ source: FRAME, data: request(name, name, {}) })
    }
    expect(calls).toEqual([])
    expect(posted.map((m) => (m as { error: { code: number; message: string } }).error)).toEqual(
      ['ui/message', 'ui/update-model-context', 'vos/toast', 'tools/call', 'resources/read'].map(
        (name) => ({
          code: -32601,
          message: `This host does not offer ${name}`
        })
      )
    )
  })

  test('a page that cannot arrange windows (the standalone one) reports no UI and keeps the rest', async () => {
    const posted: ToFrame[] = []
    const bridge = createHostBridge({
      appId: 'notes',
      frame: () => FRAME,
      post: (m) => posted.push(m),
      call: async () => null,
      services: baseServices({
        context: () =>
          context({ displayMode: 'fullscreen', availableDisplayModes: ['fullscreen'] }),
        openLink: () => {}
      }),
      context: () => context({ displayMode: 'fullscreen', availableDisplayModes: ['fullscreen'] })
    })
    bridge.receive({
      source: FRAME,
      data: request(1, 'ui/initialize', { protocolVersion: '1', appCapabilities: {} })
    })
    const result = (posted[0] as { result: { hostCapabilities: unknown } }).result
    expect(result.hostCapabilities).toEqual({
      openLinks: {},
      experimental: { vos: { backend: {}, events: {} } }
    })
    bridge.receive({
      source: FRAME,
      data: request(2, 'ui/request-display-mode', { mode: 'inline' })
    })
    await new Promise((r) => setTimeout(r, 0))
    expect(posted[1]).toEqual({ jsonrpc: '2.0', id: 2, result: { mode: 'fullscreen' } })
  })

  test('a service only the shell registers is served there, and the page that lacks it says so in words', async () => {
    const shell = rig(undefined, (base) => base.register('vos/toast', () => ({ shown: true })))
    shell.bridge.receive({ source: FRAME, data: request(1, 'vos/toast', {}) })
    await shell.flush()
    expect(shell.posted).toEqual([{ jsonrpc: '2.0', id: 1, result: { shown: true } }])
    const wrapper = rig()
    wrapper.bridge.receive({ source: FRAME, data: request(1, 'vos/toast', {}) })
    expect(wrapper.posted).toEqual([
      {
        jsonrpc: '2.0',
        id: 1,
        error: { code: -32601, message: 'This host does not offer vos/toast' }
      }
    ])
  })

  test('a service registered once is the one both host pages serve', () => {
    const registry = new ServiceRegistry().register('vos/x', () => 1)
    expect(() => registry.register('vos/x', () => 2)).toThrow('registered twice')
  })
})

describe('the services both host pages offer', () => {
  test('ui/open-link opens an http(s) link and refuses anything else with the specification’s code', async () => {
    const { bridge, posted, opened, flush } = rig()
    bridge.receive({
      source: FRAME,
      data: request(1, 'ui/open-link', { url: 'https://example.com/a?b=1' })
    })
    bridge.receive({
      source: FRAME,
      data: request(2, 'ui/open-link', { url: 'javascript:alert(1)' })
    })
    bridge.receive({ source: FRAME, data: request(3, 'ui/open-link', { url: 'not a url' }) })
    bridge.receive({ source: FRAME, data: request(4, 'ui/open-link', {}) })
    await flush()
    expect(opened).toEqual(['https://example.com/a?b=1'])
    const codes = posted.map((m) => ('error' in m ? m.error.code : 0))
    expect(codes).toEqual([0, -32000, -32000, -32602])
  })

  test('ui/request-display-mode answers with the mode it ended in, and the current one when it cannot', async () => {
    const { bridge, posted, flush } = rig()
    bridge.receive({
      source: FRAME,
      data: request(1, 'ui/request-display-mode', { mode: 'fullscreen' })
    })
    bridge.receive({ source: FRAME, data: request(2, 'ui/request-display-mode', { mode: 'pip' }) })
    bridge.receive({
      source: FRAME,
      data: request(3, 'ui/request-display-mode', { mode: 'sideways' })
    })
    await flush()
    expect(posted[0]).toEqual({ jsonrpc: '2.0', id: 1, result: { mode: 'fullscreen' } })
    expect(posted[1]).toEqual({ jsonrpc: '2.0', id: 2, result: { mode: 'fullscreen' } }) // pip is not available: the current mode
    expect((posted[2] as { error: { code: number } }).error.code).toBe(-32602)
  })
})

describe('what comes from the page and what goes into the frame', () => {
  test('a message from another window is ignored', async () => {
    const { bridge, calls, posted, events, flush } = rig()
    bridge.receive({ source: { other: true }, data: request(1, 'list', {}) })
    bridge.receive({
      source: { other: true },
      data: { jsonrpc: '2.0', method: 'ui/notifications/initialized' }
    })
    bridge.receive({ source: null, data: request(1, 'list') })
    await flush()
    expect(calls).toEqual([])
    expect(posted).toEqual([])
    expect(events).toEqual({ ready: 0, failed: [] })
  })

  test('a call that cannot be read is answered, so the page’s promise settles', async () => {
    const { bridge, posted, calls, flush } = rig()
    bridge.receive({ source: FRAME, data: { jsonrpc: '2.0', id: 1, method: '' } })
    bridge.receive({ source: FRAME, data: { jsonrpc: '2.0', id: 2, method: 'x'.repeat(129) } })
    bridge.receive({ source: FRAME, data: { jsonrpc: '2.0', id: 3, method: 7 } })
    await flush()
    expect(calls).toEqual([])
    expect(posted).toEqual([
      {
        jsonrpc: '2.0',
        id: 1,
        error: { code: -32600, message: 'A method name is 1 to 128 characters' }
      },
      {
        jsonrpc: '2.0',
        id: 2,
        error: { code: -32600, message: 'A method name is 1 to 128 characters' }
      },
      {
        jsonrpc: '2.0',
        id: 3,
        error: { code: -32600, message: 'That is not a call: it needs a method name' }
      }
    ])
  })

  test('messages that are not calls at all are dropped', async () => {
    const { bridge, calls, posted, flush } = rig()
    for (const data of [
      null,
      'rpc',
      { t: 'rpc', id: 1, method: 'list' },
      { jsonrpc: '1.0', id: 1, method: 'list' },
      { jsonrpc: '2.0', id: 1 },
      { jsonrpc: '2.0', id: 1.5, method: 'list' },
      { jsonrpc: '2.0', id: -1, method: 7 }
    ])
      bridge.receive({ source: FRAME, data })
    await flush()
    expect(calls).toEqual([])
    expect(posted).toEqual([])
  })

  test('a call whose arguments are too big or too deep is refused before it reaches the connection', async () => {
    const { bridge, posted, calls, flush } = rig()
    bridge.receive({
      source: FRAME,
      data: request(1, 'add', { title: 'x'.repeat(2 * 1024 * 1024) })
    })
    let deep: unknown = 1
    for (let i = 0; i < 150; i++) deep = [deep]
    bridge.receive({ source: FRAME, data: request(2, 'add', deep) })
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    bridge.receive({ source: FRAME, data: request(3, 'add', cyclic) })
    await flush()
    expect(calls, 'nothing reached the host connection').toEqual([])
    expect(posted.map((m) => (m as { error: { code: number; message: string } }).error)).toEqual([
      { code: -32602, message: expect.stringContaining('a call may carry at most 1,000,000') },
      { code: -32602, message: 'The arguments are nested more than 100 levels deep' },
      { code: -32602, message: expect.stringContaining('The arguments cannot be sent') }
    ])
  })

  test('arguments just under the limit go through', async () => {
    const { bridge, calls, flush } = rig()
    bridge.receive({
      source: FRAME,
      data: request(1, 'add', { title: 'x'.repeat(MAX_ARGS_BYTES - 100) })
    })
    await flush()
    expect(calls.length).toBe(1)
  })

  test('events and context changes go into the frame in the specification’s notifications', () => {
    const { bridge, posted } = rig()
    bridge.event('tasks.changed', { n: 1 })
    bridge.contextChanged({ theme: 'dark' })
    bridge.contextChanged({})
    expect(posted).toEqual([
      { jsonrpc: '2.0', method: 'vos/event', params: { name: 'tasks.changed', data: { n: 1 } } },
      { jsonrpc: '2.0', method: 'ui/notifications/host-context-changed', params: { theme: 'dark' } }
    ])
  })

  test('the first thing the page says, ui/initialize, is its hello, whether or not it was a good one', () => {
    let hellos = 0
    const bridge = createHostBridge({
      appId: 'notes',
      frame: () => FRAME,
      post: () => {},
      call: async () => null,
      services: baseServices({ context: () => context(), openLink: () => {} }),
      context: () => context(),
      onHello: () => hellos++
    })
    bridge.receive({
      source: FRAME,
      data: request(1, 'ui/initialize', { protocolVersion: '1', appCapabilities: {} })
    })
    bridge.receive({ source: FRAME, data: request(2, 'ui/initialize', { protocolVersion: '1' }) })
    bridge.receive({
      source: { other: true },
      data: request(3, 'ui/initialize', { protocolVersion: '1', appCapabilities: {} })
    })
    expect(hellos).toBe(2)
  })

  test('a burst of calls completes through the gate, never with more in flight than it allows', async () => {
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
      bridge.receive({ source: FRAME, data: request(id, 'echo', {}) })
    for (let i = 0; i < 200 && posted.length < 200; i++) await new Promise((r) => setTimeout(r, 5))
    expect(posted.length).toBe(200)
    expect(posted.every((m) => 'result' in m)).toBe(true)
    expect(peak).toBeLessThanOrEqual(MAX_IN_FLIGHT_PER_FRAME)
  })

  test('teardown asks the frame and settles on its answer, or after the wait', async () => {
    const { bridge, posted } = rig()
    const done = bridge.teardown('closed')
    const asked = posted[0] as { id: number; method: string; params: unknown }
    expect(asked).toMatchObject({
      jsonrpc: '2.0',
      method: 'ui/resource-teardown',
      params: { reason: 'closed' }
    })
    bridge.receive({ source: FRAME, data: { jsonrpc: '2.0', id: asked.id, result: {} } })
    await done
    const slow = rig().bridge.teardown('closed', 5)
    await slow
  })

  test('a disposed bridge neither acts nor answers', async () => {
    const { bridge, posted, calls, flush } = rig(
      () => new Promise((r) => setTimeout(() => r('late'), 20))
    )
    bridge.receive({ source: FRAME, data: request(1, 'list') })
    await flush()
    bridge.dispose()
    bridge.receive({ source: FRAME, data: request(2, 'list') })
    bridge.event('changed', null)
    await new Promise((r) => setTimeout(r, 40))
    expect(calls.length).toBe(1)
    expect(posted).toEqual([])
  })
})

describe('the host context', () => {
  test('comes from the appearance and the modes the page can show, with the browser’s locale and zone', () => {
    const c = hostContextOf({
      appearance: 'dark',
      displayMode: 'fullscreen',
      availableDisplayModes: ['fullscreen']
    })
    expect(c.theme).toBe('dark')
    expect(c.displayMode).toBe('fullscreen')
    expect(c.availableDisplayModes).toEqual(['fullscreen'])
    expect(c.platform).toBe('web')
    expect(c.timeZone.length).toBeGreaterThan(0)
    expect(c.locale.length).toBeGreaterThan(0)
  })

  test('a change is the fields that differ', () => {
    const before = context()
    expect(contextChange(before, { ...before, theme: 'dark' })).toEqual({ theme: 'dark' })
    expect(contextChange(before, before)).toEqual({})
    expect(
      contextChange(before, {
        ...before,
        availableDisplayModes: ['fullscreen'],
        displayMode: 'fullscreen'
      })
    ).toEqual({ availableDisplayModes: ['fullscreen'], displayMode: 'fullscreen' })
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
