/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { FakeTimers } from '../host/clock'
import { createHarness } from '../host/harness'
import { Session } from './session'
import type { RowsData } from './store'
import { createMemoryLink } from '../host/wire'
import type { ThemeSet } from '../protocol'

async function setup() {
  const timers = new FakeTimers()
  const themes: ThemeSet[] = []
  const errors: unknown[] = []
  const h = createHarness({
    timers,
    onTheme: (t) => themes.push(t),
    onError: (e) => errors.push(e)
  })
  const view = await h.openNotes()
  const received = (method: string) => h.host.received.filter((r) => r.method === method)
  return { h, view, timers, themes, errors, received, store: view.store }
}

describe('going offline', () => {
  test('the view stays mounted and goes offline; the session reports it', async () => {
    const { h, view, timers } = await setup()
    const states: string[] = []
    h.session.onState((s) => states.push(s))
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    expect(h.session.state).toBe('offline')
    expect(view.store.link).toBe('offline')
    expect(view.store.phase).toBe('ready')
    expect(view.store.tree).not.toBeNull()
    expect(states).toEqual(['offline'])
  })

  test('inputs still take drafts; nothing is sent while offline', async () => {
    const { h, view, timers, received } = await setup()
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    view.write('state.body', 'typed offline')
    view.write('src.note.body', 'edited offline')
    await timers.advance(5000)
    expect(view.resolve('state.body')).toBe('typed offline')
    expect(view.store.dirty['state:body']).toBe(true)
    expect(received('state.set')).toHaveLength(0)
    expect(received('row.set')).toHaveLength(0)
  })

  test('events are refused, not queued: a tap is not idempotent', async () => {
    const { h, view, timers, received } = await setup()
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    const res = await view.emit('main/_0/actions/save', 'tap')
    expect(res).toEqual({ ok: false, reason: 'offline' })
    expect(view.store.notices.at(-1)).toMatchObject({
      kind: 'error',
      text: 'You are offline. That action was not sent.'
    })
    h.link.setReachable(true)
    await timers.advance(5000)
    expect(received('view.event')).toHaveLength(0) // never replayed
  })

  test('client state writes work offline and are mirrored once, after the reconnect', async () => {
    const { h, view, timers, received } = await setup()
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    view.write('client.noteId', 2)
    view.write('client.noteId', 3)
    expect(view.store.client.noteId).toBe(3) // instant, local
    await timers.advance(1000)
    expect(received('client.state')).toHaveLength(0)
    h.link.setReachable(true)
    await timers.advance(5000)
    expect(received('client.state')).toHaveLength(1)
    expect(received('client.state')[0].params).toMatchObject({ set: { noteId: 3 } })
  })
})

describe('reconnect with backoff and resync', () => {
  test('retries with growing delays, then resumes the same instance and replays drafts on their base', async () => {
    const { h, view, timers, received } = await setup()
    const viewId = view.viewId
    view.write('state.body', 'draft')
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    h.host.setState('body', '+agent') // changed while the client was away
    await timers.advance(500) // first retry fails
    await timers.advance(1000) // second retry fails
    expect(h.session.state).toBe('offline')
    h.link.setReachable(true)
    await timers.advance(2000)
    expect(h.session.state).toBe('online')
    expect(view.store.link).toBe('online')
    expect(view.viewId).toBe(viewId) // the host was alive: same instance
    expect(h.session.host?.resumed).toBe(true)
    const hello = received('session.hello').at(-1)!.params as {
      resume: { sessionId: string; views: { viewId: string }[] }
    }
    expect(hello.resume).toMatchObject({ sessionId: 's1', views: [{ viewId }] })
    const open = received('view.open').at(-1)!.params as {
      resume: { viewId: string; drafts: unknown[] }
    }
    expect(open.resume.viewId).toBe(viewId)
    expect(open.resume.drafts).toEqual([
      { kind: 'state', path: 'body', value: 'draft', baseRev: 1 }
    ])
    // The draft replays with the baseRev it was typed on; the host merges on that base.
    expect(received('state.set').at(-1)!.params).toMatchObject({ value: 'draft', baseRev: 1 })
    expect(h.host.stateValue('body')).toBe('+agentdraft')
    expect(view.resolve('state.body')).toBe('+agentdraft')
  })

  test('the resumed snapshot goes through the same shielding as live patches', async () => {
    const { h, view, timers } = await setup()
    view.write('state.body', 'draft')
    h.link.drop()
    await timers.settle()
    h.host.setState('body', 'agent wrote this')
    await timers.advance(500)
    // After the snapshot lands, before the replay completes, the draft still shows.
    expect(view.store.link).toBe('online')
    expect(h.host.stateValue('body')).not.toBe('agent wrote this') // merged with the replayed draft
  })

  test('a source whose params changed offline is re-queried after the resume', async () => {
    const { h, view, timers, received } = await setup()
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    view.write('client.search', 'idea')
    await timers.advance(1000) // the debounce fires offline: remembered, not sent
    expect(received('source.set')).toHaveLength(0)
    h.link.setReachable(true)
    await timers.advance(5000)
    expect(received('source.set').at(-1)!.params).toMatchObject({
      source: 'notes',
      params: { q: 'idea' }
    })
    expect((view.store.sources.notes as RowsData).keys).toEqual([2])
  })

  test('data that changed while away arrives with the resumed snapshot', async () => {
    const { h, view, timers } = await setup()
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    h.app.agentAddNote(h.host, 'While you were away')
    h.link.setReachable(true)
    await timers.advance(5000)
    expect((view.store.sources.notes as RowsData).total).toBe(3)
  })

  test('a host restart gives a fresh instance that adopts the client persisted values', async () => {
    const { h, view, timers, received } = await setup()
    const old = view.viewId
    view.write('client.search', 'grocer')
    view.write('client.noteId', 2)
    await timers.advance(150)
    h.link.setReachable(false)
    h.link.drop()
    await timers.settle()
    h.host.restart()
    h.link.setReachable(true)
    await timers.advance(5000)
    expect(h.session.host?.resumed).toBe(false) // a new session
    expect(view.viewId).not.toBe(old)
    expect(view.store.client).toMatchObject({ search: 'grocer', noteId: 2 }) // selection survives without app code
    expect(received('view.open').at(-1)!.params).toMatchObject({
      resume: { client: { search: 'grocer', noteId: 2 } }
    })
    // Notifications route to the new id.
    h.app.agentAddNote(h.host, 'After restart')
    await timers.settle()
    expect((view.store.sources.notes as RowsData).total).toBeGreaterThan(0)
  })

  test('a write that was in flight when the socket dropped is replayed, and replay is idempotent', async () => {
    const { h, view, timers, received } = await setup()
    h.link.setLatency(100)
    view.write('state.body', 'hello')
    await timers.advance(300) // the write leaves
    await timers.advance(100) // the host has applied it, but the ack is still on its way back
    expect(h.host.stateValue('body')).toBe('hello')
    h.link.drop() // dropped before the ack arrives
    await timers.settle()
    h.link.setLatency(0)
    await timers.advance(5000)
    expect(view.store.dirty['state:body']).toBeUndefined()
    expect(h.host.stateValue('body')).toBe('hello')
    expect(received('state.set').length).toBeGreaterThanOrEqual(2)
    expect(view.resolve('state.body')).toBe('hello')
  })
})

describe('the handshake and routing', () => {
  test('a host older than this client supports is reported, not half-served', async () => {
    const timers = new FakeTimers()
    const link = createMemoryLink({ timers })
    link.onConnection((c) => {
      c.onMessage((m) => {
        const req = JSON.parse(m) as { id: number }
        c.send(
          JSON.stringify({
            jsonrpc: '2.0',
            id: req.id,
            result: { proto: 0, kit: '0.1', hostVersion: 'ancient', sessionId: 'x', resumed: false }
          })
        )
      })
      c.onClose(() => undefined)
    })
    const session = new Session({
      transport: link.transport,
      locale: 'en',
      timeZone: 'UTC',
      timers
    })
    session.start()
    await timers.settle()
    expect(session.state).toBe('incompatible')
    expect(session.online).toBe(false)
  })

  test('theme.set is session-wide', async () => {
    const { h, themes, timers } = await setup()
    const theme: ThemeSet = {
      scheme: 'dark',
      tint: 'blue',
      textScale: 110,
      reduceMotion: false,
      reduceTransparency: true,
      increaseContrast: false
    }
    h.host.allViews()[0].conn!.peer.notify('theme.set', theme as unknown as Record<string, unknown>)
    await timers.settle()
    expect(themes).toEqual([theme])
  })

  test('a notification for an unknown view is ignored', async () => {
    const { h, view, timers } = await setup()
    h.host.allViews()[0].conn!.peer.notify('view.ops', { viewId: 'nope', base: 1, rev: 2, ops: [] })
    await timers.settle()
    expect(view.store.rev).toBe(1)
  })

  test('two windows are two instances with their own client state, sharing server state', async () => {
    const { h, view, timers } = await setup()
    const other = await h.openNotes({ windowId: 'w2' })
    expect(other.viewId).not.toBe(view.viewId)
    view.write('client.search', 'idea')
    await timers.advance(150)
    expect((view.store.sources.notes as RowsData).total).toBe(1)
    expect((other.store.sources.notes as RowsData).total).toBe(2) // its own search
    h.app.agentAppendDraft(h.host, 'shared')
    await timers.settle()
    expect(view.resolve('state.body')).toBe('shared')
    expect(other.resolve('state.body')).toBe('shared') // one app state
  })

  test('stopping says goodbye and stays closed', async () => {
    const { h, timers, received } = await setup()
    h.session.stop()
    await timers.advance(10_000)
    expect(received('session.bye')).toHaveLength(1)
    expect(h.link.connections).toBe(1)
  })
})
