/**
 * Device pairing over plain HTTP (docs/architecture/host-stack.md, "Pairing"): `GET /session` says whether this
 * browser is paired, `POST /pair` trades the code the host printed for a session cookie. Both are unauthenticated
 * by design; the cookie is HttpOnly, so the page never sees the token.
 */

type FetchFn = typeof fetch

export async function isPaired(fetchFn: FetchFn = fetch): Promise<boolean> {
  const res = await fetchFn('/session', { credentials: 'same-origin', cache: 'no-store' })
  if (!res.ok) throw new Error(`The host answered ${res.status}`)
  const body = (await res.json()) as { paired?: boolean }
  return body.paired === true
}

export type PairResult =
  | { ok: true }
  | {
      ok: false
      reason: 'invalid' | 'rate-limited' | 'unavailable'
      message: string
      retryAfterSecs?: number
    }

/** Normalise what a person typed or pasted: case and separators do not matter (`k7qm 3xhr` is `K7QM-3XHR`). */
export function normaliseCode(input: string): string {
  const letters = input.toUpperCase().replace(/[^A-Z0-9]/g, '')
  return letters.length > 4 ? `${letters.slice(0, 4)}-${letters.slice(4, 8)}` : letters
}

export async function pairDevice(
  code: string,
  deviceName: string,
  fetchFn: FetchFn = fetch
): Promise<PairResult> {
  let res: Response
  try {
    res = await fetchFn('/pair', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        code,
        ...(deviceName.trim() ? { deviceName: deviceName.trim() } : {})
      })
    })
  } catch {
    return {
      ok: false,
      reason: 'unavailable',
      message: 'Could not reach the host. Check that it is running and try again.'
    }
  }
  if (res.ok) return { ok: true }
  if (res.status === 401)
    return {
      ok: false,
      reason: 'invalid',
      message:
        'That code did not work. It may be mistyped, already used, or older than 10 minutes. Run “agentos-host pair” for a new one.'
    }
  if (res.status === 429) {
    const header = Number(res.headers.get('retry-after'))
    const body = (await res.json().catch(() => ({}))) as { retryAfterSecs?: number }
    const seconds = Number.isFinite(header) && header > 0 ? header : (body.retryAfterSecs ?? 60)
    return {
      ok: false,
      reason: 'rate-limited',
      retryAfterSecs: seconds,
      message: `Too many attempts. Try again in ${seconds} ${seconds === 1 ? 'second' : 'seconds'}.`
    }
  }
  return {
    ok: false,
    reason: 'unavailable',
    message: `The host could not pair this device (${res.status}).`
  }
}
