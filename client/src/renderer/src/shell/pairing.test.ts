import { describe, expect, test } from 'bun:test'
import { isPaired, normaliseCode, pairDevice } from './pairing'

const respond = (
  status: number,
  body: unknown,
  headers: Record<string, string> = {}
): typeof fetch =>
  (async () =>
    new Response(JSON.stringify(body), {
      status,
      headers: { 'content-type': 'application/json', ...headers }
    })) as unknown as typeof fetch

describe('pairing', () => {
  test('GET /session says whether this browser is paired', async () => {
    expect(await isPaired(respond(200, { paired: false }))).toBe(false)
    expect(await isPaired(respond(200, { paired: true, device: {} }))).toBe(true)
    await expect(isPaired(respond(500, {}))).rejects.toThrow('500')
  })

  test('a paired device continues; the body carries the code and an optional name', async () => {
    let sent = ''
    const fetchFn = (async (_url: string, init: RequestInit) => {
      sent = String(init.body)
      return new Response('{}', { status: 200 })
    }) as unknown as typeof fetch
    expect(await pairDevice('K7QM-3XHR', ' My iPad ', fetchFn)).toEqual({ ok: true })
    expect(JSON.parse(sent)).toEqual({ code: 'K7QM-3XHR', deviceName: 'My iPad' })
    await pairDevice('K7QM-3XHR', '', fetchFn)
    expect(JSON.parse(sent)).toEqual({ code: 'K7QM-3XHR' })
  })

  test('a wrong or expired code is one friendly message', async () => {
    const result = await pairDevice('AAAA-BBBB', '', respond(401, { error: 'x' }))
    expect(result).toMatchObject({ ok: false, reason: 'invalid' })
    expect(result.ok === false && result.message).toContain('10 minutes')
  })

  test('rate limiting reads Retry-After', async () => {
    const result = await pairDevice('AAAA-BBBB', '', respond(429, {}, { 'retry-after': '42' }))
    expect(result).toMatchObject({ ok: false, reason: 'rate-limited', retryAfterSecs: 42 })
    expect(result.ok === false && result.message).toBe(
      'Too many attempts. Try again in 42 seconds.'
    )
    const body = await pairDevice('A', '', respond(429, { retryAfterSecs: 1 }))
    expect(body.ok === false && body.message).toBe('Too many attempts. Try again in 1 second.')
  })

  test('an unreachable host is its own message', async () => {
    const down = (async () => {
      throw new TypeError('fetch failed')
    }) as unknown as typeof fetch
    expect(await pairDevice('AAAA-BBBB', '', down)).toMatchObject({
      ok: false,
      reason: 'unavailable'
    })
  })

  test('typing is forgiving: case, spaces and the dash', () => {
    expect(normaliseCode('k7qm 3xhr')).toBe('K7QM-3XHR')
    expect(normaliseCode('K7QM-3XHR')).toBe('K7QM-3XHR')
    expect(normaliseCode('k7q')).toBe('K7Q')
    expect(normaliseCode('k7qm3xhrzz')).toBe('K7QM-3XHR')
  })
})
