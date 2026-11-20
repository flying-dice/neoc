/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { createRpcPeer, isConnectionError, RpcError, RpcErrorCode, type RpcPeer } from './peer'

/** Two peers wired back to back (frames are real strings). */
function pair(
  handlers: {
    a?: Parameters<typeof createRpcPeer>[0]
    b?: Parameters<typeof createRpcPeer>[0]
  } = {}
) {
  const frames: string[] = []
  const ends: { a?: RpcPeer; b?: RpcPeer } = {}
  const a = createRpcPeer({
    ...handlers.a,
    send: (m) => (frames.push(m), queueMicrotask(() => ends.b?.receive(m)))
  })
  const b = createRpcPeer({
    ...handlers.b,
    send: (m) => (frames.push(m), queueMicrotask(() => ends.a?.receive(m)))
  })
  ends.a = a
  ends.b = b
  return { a, b, frames }
}

describe('json-rpc peer (by-name params)', () => {
  test('a request carries a params object and resolves with the result', async () => {
    const { a, frames } = pair({
      b: { send: () => undefined, onRequest: (m, p) => ({ method: m, echoed: p }) }
    })
    const result = await a.request('view.sync', { viewId: 'v1' })
    expect(result).toEqual({ method: 'view.sync', echoed: { viewId: 'v1' } })
    expect(JSON.parse(frames[0])).toMatchObject({
      jsonrpc: '2.0',
      method: 'view.sync',
      params: { viewId: 'v1' }
    })
  })

  test('a thrown RpcError keeps its code and data; other errors become InternalError', async () => {
    const { a } = pair({
      b: {
        send: () => undefined,
        onRequest: (method) => {
          if (method === 'app') throw new RpcError(-32010, 'No such note', { id: 7 })
          throw new Error('boom')
        }
      }
    })
    const app = await a.request('app').catch((e: RpcError) => e)
    expect(app).toBeInstanceOf(RpcError)
    expect(app).toMatchObject({ code: -32010, message: 'No such note', data: { id: 7 } })
    const other = await a.request('other').catch((e: RpcError) => e)
    expect(other).toMatchObject({ code: RpcErrorCode.InternalError, message: 'boom' })
  })

  test('an unhandled method answers MethodNotFound', async () => {
    const { a } = pair()
    const e = await a.request('nope').catch((x: RpcError) => x)
    expect(e).toMatchObject({ code: RpcErrorCode.MethodNotFound })
  })

  test('notifications have no id, reach the other side, and handler errors are reported not thrown', async () => {
    const seen: unknown[] = []
    const errors: unknown[] = []
    const { a, frames } = pair({
      b: {
        send: () => undefined,
        onNotification: (m, p) => {
          seen.push([m, p])
          throw new Error('handler failed')
        },
        onError: (e) => errors.push(e)
      }
    })
    a.notify('view.visibility', { viewId: 'v1', visible: false })
    await Promise.resolve()
    await Promise.resolve()
    expect(seen).toEqual([['view.visibility', { viewId: 'v1', visible: false }]])
    expect(JSON.parse(frames[0])).not.toHaveProperty('id')
    expect(errors).toHaveLength(1)
  })

  test('array params are refused: this protocol is by-name', async () => {
    const out: string[] = []
    const peer = createRpcPeer({ send: (m) => out.push(m), onRequest: () => 'ok' })
    peer.receive(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'x', params: [1, 2] }))
    await Promise.resolve()
    expect(JSON.parse(out[0]).error.code).toBe(RpcErrorCode.InvalidParams)
  })

  test('garbage frames are answered with ParseError / InvalidRequest and never throw', () => {
    const out: string[] = []
    const peer = createRpcPeer({ send: (m) => out.push(m) })
    peer.receive('not json')
    peer.receive(JSON.stringify({ hello: 'world' }))
    expect(out.map((m) => JSON.parse(m).error.code)).toEqual([
      RpcErrorCode.ParseError,
      RpcErrorCode.InvalidRequest
    ])
  })

  test('close rejects in-flight requests with ConnectionClosed and refuses new ones', async () => {
    const peer = createRpcPeer({ send: () => undefined })
    const inflight = peer.request('slow').catch((e: RpcError) => e)
    peer.close()
    expect(await inflight).toMatchObject({ code: RpcErrorCode.ConnectionClosed })
    expect(isConnectionError(await inflight)).toBe(true)
    expect(await peer.request('again').catch((e: RpcError) => e)).toMatchObject({
      code: RpcErrorCode.ConnectionClosed
    })
  })

  test('a request times out with Timeout, which counts as a connection error', async () => {
    const peer = createRpcPeer({ send: () => undefined, timeoutMs: 5 })
    const e = await peer.request('hang').catch((x: RpcError) => x)
    expect(e).toMatchObject({ code: RpcErrorCode.Timeout })
    expect(isConnectionError(e)).toBe(true)
  })

  test('application errors are not connection errors', () => {
    expect(isConnectionError(new RpcError(-32010, 'nope'))).toBe(false)
    expect(isConnectionError(new Error('x'))).toBe(false)
  })

  test('an id prefix keeps two peers on one connection apart', async () => {
    const sent: string[] = []
    const peer = createRpcPeer({ send: (m) => void sent.push(m), idPrefix: 's' })
    const reply = peer.request('app.list')
    expect(JSON.parse(sent[0]).id).toBe('s1')
    peer.receive(JSON.stringify({ jsonrpc: '2.0', id: 's1', result: { ok: true } }))
    expect(await reply).toEqual({ ok: true })
  })

  test('failPending rejects what is in flight but the peer keeps working (the socket may come back)', async () => {
    const sent: string[] = []
    const peer = createRpcPeer({ send: (m) => void sent.push(m) })
    const inflight = peer.request('a').catch((e: RpcError) => e)
    peer.failPending('dropped')
    expect(await inflight).toMatchObject({
      code: RpcErrorCode.ConnectionClosed,
      message: 'dropped'
    })
    const again = peer.request('b')
    peer.receive(JSON.stringify({ jsonrpc: '2.0', id: 2, result: 1 }))
    expect(await again).toBe(1)
  })

  test('a transport that refuses the frame rejects the request at once and leaves nothing pending', async () => {
    const peer = createRpcPeer({
      send: () => {
        throw new Error('not open')
      },
      timeoutMs: 20
    })
    const e = await peer.request('x').catch((x: RpcError) => x)
    expect(e).toMatchObject({ code: RpcErrorCode.ConnectionClosed, message: 'not open' })
    // the late timer must not fire a second rejection or throw
    await new Promise((r) => setTimeout(r, 40))
  })
})
