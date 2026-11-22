/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { FakeTimers } from './rpc/fakeTimers'
import { createMemoryLink } from './rpc/memoryLink'
import { Session, type SessionState } from './session'

interface Frame {
  id?: number
  method: string
  params: Record<string, unknown>
}

/** A session over an in-memory wire whose host end answers `session.hello` (with `welcome`, or as `answer` says). */
function setup(
  options: {
    welcome?: Record<string, unknown>
    answer?: (frame: Frame) => Record<string, unknown> | null
  } = {}
) {
  const timers = new FakeTimers()
  const link = createMemoryLink({ timers })
  const received: Frame[] = []
  const errors: unknown[] = []
  let sessions = 0
  let known: string | null = null
  link.onConnection((c) => {
    c.onMessage((m) => {
      const frame = JSON.parse(m) as Frame
      received.push(frame)
      if (frame.id === undefined || frame.method !== 'session.hello') return
      const custom = options.answer?.(frame)
      if (custom === null) return // the host never answers
      const resume = frame.params.resume as { sessionId: string } | undefined
      const resumed = resume !== undefined && resume.sessionId === known
      if (!resumed) known = `s${++sessions}`
      const result = {
        proto: 1,
        kit: '1.0',
        hostVersion: 'test',
        sessionId: known,
        resumed,
        ...options.welcome
      }
      c.send(JSON.stringify({ jsonrpc: '2.0', id: frame.id, ...(custom ?? { result }) }))
    })
    c.onClose(() => undefined)
  })
  const session = new Session({
    transport: link.transport,
    locale: 'en-GB',
    timeZone: 'UTC',
    onError: (e) => errors.push(e)
  })
  const states: SessionState[] = []
  session.onState((s) => states.push(s))
  const hellos = () => received.filter((r) => r.method === 'session.hello')
  return {
    timers,
    link,
    session,
    states,
    errors,
    received,
    hellos,
    /** Forget every session, as a host that restarted does. */
    restartHost: () => void (known = null)
  }
}

async function online(s = setup()) {
  s.session.start()
  await s.timers.settle()
  return s
}

describe('the handshake', () => {
  test('says hello with the protocol, the kit and the formatting environment, and goes online', async () => {
    const s = await online()
    expect(s.hellos()).toHaveLength(1)
    expect(s.hellos()[0].params).toEqual({
      proto: 1,
      kit: expect.any(String),
      locale: 'en-GB',
      timeZone: 'UTC',
      features: []
    })
    expect(s.session.state).toBe('online')
    expect(s.session.online).toBe(true)
    expect(s.session.host).toMatchObject({ hostVersion: 'test', sessionId: 's1', resumed: false })
    expect(s.states).toEqual(['online'])
  })

  test('a host older than this client supports is reported, not half-served', async () => {
    const s = await online(setup({ welcome: { proto: 0, kit: '0.1', hostVersion: 'ancient' } }))
    expect(s.session.state).toBe('incompatible')
    expect(s.session.online).toBe(false)
  })

  test('a host that refuses the handshake is incompatible, and the error is reported', async () => {
    const s = await online(
      setup({ answer: () => ({ error: { code: -32602, message: 'unsupported proto' } }) })
    )
    expect(s.session.state).toBe('incompatible')
    expect(s.session.online).toBe(false)
    expect(s.errors).toHaveLength(1)
  })

  test('a socket that drops during the handshake is not a refusal: the session keeps connecting', async () => {
    const s = setup({ answer: () => null })
    s.session.start()
    await s.timers.settle()
    s.link.setReachable(false)
    s.link.drop()
    await s.timers.settle()
    expect(s.session.state).toBe('connecting') // never online, so not "offline"
    expect(s.errors).toEqual([])
  })

  test('an incompatible host stays incompatible when its socket drops', async () => {
    const s = await online(setup({ welcome: { proto: 0 } }))
    s.link.setReachable(false)
    s.link.drop()
    await s.timers.settle()
    expect(s.session.state).toBe('incompatible')
  })
})

describe('going offline and coming back', () => {
  test('a drop after being online is offline; the session reports it', async () => {
    const s = await online()
    s.link.setReachable(false)
    s.link.drop()
    await s.timers.settle()
    expect(s.session.state).toBe('offline')
    expect(s.session.online).toBe(false)
    expect(s.states).toEqual(['online', 'offline'])
  })

  test('retries with growing delays, then resumes the same session', async () => {
    const s = await online()
    s.link.setReachable(false)
    s.link.drop()
    await s.timers.settle()
    await s.timers.advance(500) // first retry fails
    await s.timers.advance(1000) // second retry fails
    expect(s.session.state).toBe('offline')
    s.link.setReachable(true)
    await s.timers.advance(2000)
    expect(s.session.state).toBe('online')
    expect(s.session.host?.resumed).toBe(true) // the host was alive: the same session
    expect(s.hellos().at(-1)!.params.resume).toEqual({ sessionId: 's1', views: [] })
    expect(s.states).toEqual(['online', 'offline', 'online'])
  })

  test('a host restart gives a new session, which the next reconnect resumes', async () => {
    const s = await online()
    s.link.drop()
    s.restartHost()
    await s.timers.advance(5000)
    expect(s.session.state).toBe('online')
    expect(s.session.host).toMatchObject({ sessionId: 's2', resumed: false })
    s.link.drop()
    await s.timers.advance(5000)
    expect(s.hellos().at(-1)!.params.resume).toMatchObject({ sessionId: 's2' })
    expect(s.session.host?.resumed).toBe(true)
  })

  test('retryNow skips the wait', async () => {
    const s = await online()
    s.link.setReachable(false)
    s.link.drop()
    await s.timers.settle()
    s.link.setReachable(true)
    s.session.retryNow()
    await s.timers.settle()
    expect(s.session.state).toBe('online')
  })
})

describe('stopping', () => {
  test('says goodbye and stays closed', async () => {
    const s = await online()
    s.session.stop()
    await s.timers.advance(10_000)
    expect(s.received.filter((r) => r.method === 'session.bye')).toHaveLength(1)
    expect(s.link.connections).toBe(1)
    expect(s.session.online).toBe(false)
  })
})
