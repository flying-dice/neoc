import { describe, expect, test } from 'bun:test'
import { createHostChannel, HostError } from './channel'

type Status = 'connecting' | 'open' | 'closed'

/** A transport that records what is sent and lets the test push frames in. */
const noop = (): void => undefined

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type -- the shape is the fake's own
function fakeTransport() {
  const sent: string[] = []
  const messages = new Set<(m: string) => void>()
  const statuses = new Set<(s: Status) => void>()
  return {
    sent,
    receive: (frame: object) => messages.forEach((l) => l(JSON.stringify(frame))),
    setStatus: (s: Status) => statuses.forEach((l) => l(s)),
    transport: {
      status: 'open' as Status,
      send: (m: string) => void sent.push(m),
      connect: noop,
      close: noop,
      retryNow: noop,
      onMessage: (l: (m: string) => void) => (messages.add(l), () => void messages.delete(l)),
      onStatus: (l: (s: Status) => void) => (statuses.add(l), () => void statuses.delete(l))
    }
  }
}

describe('the shell channel', () => {
  test('requests use string ids and their responses never reach the session', async () => {
    const wire = fakeTransport()
    const channel = createHostChannel(wire.transport)
    const seenBySession: string[] = []
    channel.transport.onMessage((m) => seenBySession.push(m))

    const reply = channel.call('app.list')
    expect(JSON.parse(wire.sent[0])).toMatchObject({ id: 's1', method: 'app.list', params: {} })
    wire.receive({ jsonrpc: '2.0', id: 's1', result: { apps: [1] } })
    expect(await reply).toEqual({ apps: [1] })
    expect(seenBySession).toEqual([])

    // the session's own numeric ids pass through untouched
    wire.receive({ jsonrpc: '2.0', id: 1, result: { ok: true } })
    expect(seenBySession).toHaveLength(1)
  })

  test('a JSON-RPC error rejects with the host message and code', async () => {
    const wire = fakeTransport()
    const channel = createHostChannel(wire.transport)
    const reply = channel.call('app.install', { url: 'x' })
    wire.receive({
      jsonrpc: '2.0',
      id: 's1',
      error: { code: -32004, message: 'app `x` is already installed' }
    })
    const error = await reply.catch((e: unknown) => e)
    expect(error).toBeInstanceOf(HostError)
    expect((error as HostError).message).toContain('already installed')
    expect((error as HostError).code).toBe(-32004)
  })

  test('notifications reach the shell and the session; window.open is the shell’s to act on', () => {
    const wire = fakeTransport()
    const channel = createHostChannel(wire.transport)
    const seenBySession: string[] = []
    channel.transport.onMessage((m) => seenBySession.push(m))
    const heard: [string, unknown][] = []
    channel.onNotification((method, params) => heard.push([method, params]))
    wire.receive({
      jsonrpc: '2.0',
      method: 'window.open',
      params: { app: 'calculator', window: 'main' }
    })
    expect(heard).toEqual([['window.open', { app: 'calculator', window: 'main' }]])
    expect(seenBySession).toHaveLength(1)
  })

  test('requests in flight fail when the connection closes', async () => {
    const wire = fakeTransport()
    const channel = createHostChannel(wire.transport)
    const reply = channel.call('app.list')
    wire.setStatus('closed')
    expect(await reply.catch((e: unknown) => e)).toBeInstanceOf(HostError)
  })

  test('a call on a transport that is not open rejects at once with "Not connected"', async () => {
    const wire = fakeTransport()
    wire.transport.send = () => {
      throw new Error('socket closed')
    }
    const channel = createHostChannel(wire.transport)
    const error = await channel.call('app.list').catch((e: unknown) => e)
    expect(error).toBeInstanceOf(HostError)
    expect((error as HostError).message).toBe('Not connected to AgentOS')
  })

  test('a call the host never answers times out with a HostError', async () => {
    const wire = fakeTransport()
    const channel = createHostChannel(wire.transport, 5)
    const error = await channel.call('app.list').catch((e: unknown) => e)
    expect(error).toBeInstanceOf(HostError)
    expect((error as HostError).code).toBe(-32001)
  })

  test('a request from the host is the session’s, never the shell’s', () => {
    const wire = fakeTransport()
    const channel = createHostChannel(wire.transport)
    const seenBySession: string[] = []
    channel.transport.onMessage((m) => seenBySession.push(m))
    wire.receive({ jsonrpc: '2.0', id: 's9', method: 'ui.ask', params: {} })
    expect(seenBySession).toHaveLength(1)
    expect(wire.sent).toEqual([]) // the shell did not answer "method not found" on the session's behalf
  })
})
