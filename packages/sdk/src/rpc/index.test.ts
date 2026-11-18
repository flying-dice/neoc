import { describe, expect, test } from 'bun:test'
import { createRpcPeer, RpcError, RpcErrorCode, type RpcPeer } from './index'

/** Two peers wired back to back, as over a WebSocket. */
function pair(
  serverHandlers: { onRequest?(m: string, p: unknown[]): unknown; onNotification?(m: string, p: unknown[]): void } = {},
  clientHandlers: { onNotification?(m: string, p: unknown[]): void } = {}
): { client: RpcPeer; server: RpcPeer; wire: string[] } {
  const wire: string[] = []
  let client: RpcPeer
  const server: RpcPeer = createRpcPeer({
    send: (m) => {
      wire.push(m)
      queueMicrotask(() => client.receive(m))
    },
    ...serverHandlers
  })
  client = createRpcPeer({
    send: (m) => {
      wire.push(m)
      queueMicrotask(() => server.receive(m))
    },
    ...clientHandlers
  })
  return { client, server, wire }
}

describe('json-rpc peer', () => {
  test('request resolves with the handler result and uses positional params', async () => {
    const { client, wire } = pair({ onRequest: (m, p) => (m === 'add' ? (p[0] as number) + (p[1] as number) : null) })
    expect(await client.request('add', [2, 3])).toBe(5)
    expect(JSON.parse(wire[0])).toEqual({ jsonrpc: '2.0', id: 1, method: 'add', params: [2, 3] })
  })

  test('undefined results travel as null', async () => {
    const { client } = pair({ onRequest: () => undefined })
    expect(await client.request('noop')).toBeNull()
  })

  test('handler errors become JSON-RPC errors with their code', async () => {
    const { client } = pair({
      onRequest: (m) => {
        if (m === 'bad') throw new RpcError(RpcErrorCode.InvalidParams, 'nope', { field: 'x' })
        throw new Error('boom')
      }
    })
    const bad = await client.request('bad').catch((e: RpcError) => e)
    expect(bad).toBeInstanceOf(RpcError)
    expect((bad as RpcError).code).toBe(RpcErrorCode.InvalidParams)
    expect((bad as RpcError).data).toEqual({ field: 'x' })
    const boom = (await client.request('other').catch((e: RpcError) => e)) as RpcError
    expect(boom.code).toBe(RpcErrorCode.InternalError)
    expect(boom.message).toBe('boom')
  })

  test('a peer without request handling answers MethodNotFound', async () => {
    const { client } = pair()
    const e = (await client.request('anything').catch((x: RpcError) => x)) as RpcError
    expect(e.code).toBe(RpcErrorCode.MethodNotFound)
  })

  test('notifications flow both ways without ids', async () => {
    const seen: unknown[] = []
    const { client, server } = pair(
      { onNotification: (m, p) => seen.push(['server', m, p]) },
      { onNotification: (m, p) => seen.push(['client', m, p]) }
    )
    client.notify('ping', [1])
    server.notify('event', [{ a: 1 }])
    await Bun.sleep(0)
    expect(seen).toEqual([
      ['server', 'ping', [1]],
      ['client', 'event', [{ a: 1 }]]
    ])
  })

  test('malformed frames get ParseError / InvalidRequest responses', () => {
    const sent: string[] = []
    const peer = createRpcPeer({ send: (m) => sent.push(m) })
    peer.receive('{not json')
    peer.receive(JSON.stringify({ hello: 'world' }))
    peer.receive(JSON.stringify({ jsonrpc: '2.0', id: 7, method: 'x', params: { named: true } }))
    expect(sent.map((s) => JSON.parse(s).error.code)).toEqual([
      RpcErrorCode.ParseError,
      RpcErrorCode.InvalidRequest,
      RpcErrorCode.InvalidParams
    ])
  })

  test('close rejects in-flight requests and refuses new ones', async () => {
    const peer = createRpcPeer({ send: () => {} })
    const inflight = peer.request('slow')
    peer.close('socket closed')
    expect(((await inflight.catch((e) => e)) as RpcError).code).toBe(RpcErrorCode.ConnectionClosed)
    expect(((await peer.request('x').catch((e) => e)) as RpcError).code).toBe(RpcErrorCode.ConnectionClosed)
  })

  test('requests time out', async () => {
    const peer = createRpcPeer({ send: () => {}, timeoutMs: 10 })
    const e = (await peer.request('never').catch((x) => x)) as RpcError
    expect(e.code).toBe(RpcErrorCode.Timeout)
  })
})
