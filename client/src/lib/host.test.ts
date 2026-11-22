/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { beforeEach, describe, expect, test } from 'bun:test'
import { Host, type HostDeps } from './host.svelte'
import { appOf, dockOrder, setInstalledApps } from './apps.svelte'
import { HostError } from '../shell/channel'
import type { PairResult } from '../shell/pairing'
import type { AppSummary } from '../shell/hostApi'
import type { Transport, TransportStatus } from '../view/rpc/transport'

const calculator: AppSummary = {
  id: 'calculator',
  name: 'Calculator',
  version: '0.1.0',
  state: 'running',
  source: 'file',
  client: false,
  permissions: []
}
const agents: AppSummary = { ...calculator, id: 'agents', name: 'Agents' }

type Answer = unknown | ((params: Record<string, unknown>) => unknown)

/** A wire the test drives: it answers `session.hello` and the shell's methods, and opens and drops on demand. */
function fakeWire() {
  const messages = new Set<(m: string) => void>()
  const statuses = new Set<(s: TransportStatus) => void>()
  const answers = new Map<string, Answer>()
  const calls: string[] = []
  let status: TransportStatus = 'closed'
  const wire = {
    calls,
    answers,
    transport: {
      get status() {
        return status
      },
      send: (raw: string) => {
        if (status !== 'open') throw new Error('not open')
        const frame = JSON.parse(raw) as { id?: number | string; method: string; params: object }
        if (frame.id === undefined) return
        calls.push(frame.method)
        queueMicrotask(() => {
          const answer =
            answers.get(frame.method) ??
            (frame.method === 'session.hello'
              ? { proto: 1, kit: '1', hostVersion: 't', sessionId: 'sess-1', resumed: false }
              : undefined)
          const result = typeof answer === 'function' ? answer(frame.params) : answer
          const reply =
            result instanceof Error
              ? { jsonrpc: '2.0', id: frame.id, error: { code: -32010, message: result.message } }
              : { jsonrpc: '2.0', id: frame.id, result: result ?? null }
          messages.forEach((l) => l(JSON.stringify(reply)))
        })
      },
      connect: () => undefined,
      close: () => void wire.set('closed'),
      retryNow: () => undefined,
      onMessage: (l: (m: string) => void) => (messages.add(l), () => void messages.delete(l)),
      onStatus: (l: (s: TransportStatus) => void) => (
        statuses.add(l),
        () => void statuses.delete(l)
      )
    } satisfies Transport,
    set(next: TransportStatus) {
      status = next
      statuses.forEach((l) => l(next))
    },
    notify(method: string, params: object) {
      messages.forEach((l) => l(JSON.stringify({ jsonrpc: '2.0', method, params })))
    }
  }
  return wire
}

const settle = () => new Promise<void>((r) => setTimeout(r, 5))

function setup(deps: Partial<HostDeps> = {}) {
  const wire = fakeWire()
  const scheduled: { run: () => void; ms: number }[] = []
  const paired = { value: true as boolean | Error }
  const pairing = { result: { ok: true } as PairResult, attempts: [] as string[][] }
  const host = new Host({
    isPaired: async () => {
      if (paired.value instanceof Error) throw paired.value
      return paired.value
    },
    pairDevice: async (code, name) => {
      pairing.attempts.push([code, name])
      return pairing.result
    },
    createTransport: () => wire.transport,
    schedule: (run, ms) => void scheduled.push({ run, ms }),
    ...deps
  })
  wire.answers.set('app.list', { apps: [calculator] })
  wire.answers.set('agents.list', { agents: [{ id: 'a1', name: 'A', role: '' }] })
  return { host, wire, paired, pairing, scheduled }
}

/** Start a paired host and bring the wire up until the desktop is ready. */
async function online(s = setup()) {
  await s.host.start()
  s.wire.set('open')
  await settle()
  return s
}

// the app registry is shared module state: start every test from an empty one
beforeEach(() => setInstalledApps([]))

describe('connection phases', () => {
  test('an unpaired browser goes to the pairing screen and never connects', async () => {
    const s = setup()
    s.paired.value = false
    await s.host.start()
    expect(s.host.phase).toBe('unpaired')
    expect(s.host.session).toBeNull()
    expect(s.wire.calls).toEqual([])
  })

  test('a paired browser is connecting until the host answers the handshake, then ready with its apps and agents', async () => {
    const s = setup()
    expect(s.host.phase).toBe('checking')
    await s.host.start()
    expect(s.host.phase).toBe('connecting')
    s.wire.set('open')
    await settle()
    expect(s.host.phase).toBe('ready')
    expect(s.host.connection).toBe('online')
    expect(s.host.apps.map((a) => a.id)).toEqual(['calculator'])
    expect(s.host.agents.map((a) => a.id)).toEqual(['a1'])
    expect(appOf('calculator')?.name).toBe('Calculator')
  })

  test('a host that does not answer /session is unreachable with the reason, and retry starts over', async () => {
    const s = setup()
    s.paired.value = new Error('The host answered 502')
    await s.host.start()
    expect(s.host.phase).toBe('unreachable')
    expect(s.host.error).toBe('The host answered 502')
    s.paired.value = false
    s.host.retry()
    await settle()
    expect(s.host.phase).toBe('unpaired')
  })

  test('a drop after ready keeps the desktop and says "offline"; it does not go back to pairing while the host is paired', async () => {
    const s = await online()
    s.wire.set('closed')
    await settle()
    expect(s.host.connection).toBe('offline')
    expect(s.host.phase).toBe('ready')
    expect(s.host.session).not.toBeNull()
  })

  test('a host that speaks a protocol older than the client’s is "incompatible" and the desktop never opens', async () => {
    const s = setup()
    s.wire.answers.set('session.hello', {
      proto: 0,
      kit: '1',
      hostVersion: 't',
      sessionId: 'x',
      resumed: false
    })
    await s.host.start()
    s.wire.set('open')
    await settle()
    expect(s.host.connection).toBe('incompatible')
    expect(s.host.phase).toBe('connecting')
    expect(s.wire.calls).not.toContain('app.list')
  })
})

describe('pairing', () => {
  test('a good code connects; a wrong one reports why and stays on the pairing screen', async () => {
    const s = setup()
    s.paired.value = false
    await s.host.start()
    s.pairing.result = { ok: false, reason: 'invalid', message: 'That code did not work.' }
    expect(await s.host.pair('AAAA-BBBB', 'iPad')).toMatchObject({ ok: false, reason: 'invalid' })
    expect(s.host.phase).toBe('unpaired')
    expect(s.host.session).toBeNull()
    s.pairing.result = { ok: true }
    expect(await s.host.pair('CCCC-DDDD', 'iPad')).toEqual({ ok: true })
    expect(s.host.phase).toBe('connecting')
    expect(s.pairing.attempts).toEqual([
      ['AAAA-BBBB', 'iPad'],
      ['CCCC-DDDD', 'iPad']
    ])
  })
})

describe('verifyPairing (a revoked device goes back to the pairing screen)', () => {
  test('a drop with the device no longer paired: unpaired, session gone, apps cleared', async () => {
    const s = await online()
    s.paired.value = false
    s.wire.set('closed')
    await settle()
    expect(s.host.phase).toBe('unpaired')
    expect(s.host.session).toBeNull()
    expect(s.host.apps).toEqual([])
    // the shell's registry (windows, Dock) is emptied with it, not only the host's own list
    expect(appOf('calculator')).toBeUndefined()
    expect(dockOrder()).not.toContain('calculator')
  })

  test('a drop with the device still paired keeps reconnecting', async () => {
    const s = await online()
    s.wire.set('closed')
    await settle()
    expect(s.host.phase).toBe('ready')
  })

  test('a drop while /session itself is unreachable is not a revocation', async () => {
    const s = await online()
    s.paired.value = new Error('network down')
    s.wire.set('closed')
    await settle()
    expect(s.host.phase).toBe('ready')
    expect(s.host.session).not.toBeNull()
  })

  test('a socket that never opens is checked once after the grace period, and a revoked device is unpaired', async () => {
    const s = setup()
    await s.host.start()
    expect(s.scheduled).toHaveLength(1)
    expect(s.scheduled[0].ms).toBe(3000)
    s.paired.value = false
    s.scheduled[0].run()
    await settle()
    expect(s.host.phase).toBe('unpaired')
  })

  test('the grace-period check does nothing once the session is online', async () => {
    const s = await online()
    s.paired.value = false
    s.scheduled[0].run()
    await settle()
    expect(s.host.phase).toBe('ready')
  })
})

describe('refresh coalescing', () => {
  test('calls made while one runs collapse into one more run, and every caller waits for the last', async () => {
    const s = await online()
    const before = s.wire.calls.filter((m) => m === 'app.list').length
    let n = 0
    s.wire.answers.set('app.list', () => ({
      apps: [{ ...calculator, name: `run ${++n}` }]
    }))
    const all = [s.host.refresh(), s.host.refresh(), s.host.refresh(), s.host.refresh()]
    await Promise.all(all)
    expect(s.wire.calls.filter((m) => m === 'app.list').length - before).toBe(2)
    expect(s.host.apps[0].name).toBe('run 2')
  })

  test('a refresh after the last one finished starts a fresh run', async () => {
    const s = await online()
    const before = s.wire.calls.filter((m) => m === 'app.list').length
    await s.host.refresh()
    await s.host.refresh()
    expect(s.wire.calls.filter((m) => m === 'app.list').length - before).toBe(2)
  })

  test('an app lifecycle notification re-reads the list (installed, uninstalled, enabled, disabled, failed)', async () => {
    const s = await online()
    for (const method of [
      'app.installed',
      'app.uninstalled',
      'app.enabled',
      'app.disabled',
      'app.failed'
    ]) {
      const before = s.wire.calls.filter((m) => m === 'app.list').length
      s.wire.notify(method, { app: 'x' })
      await settle()
      expect(s.wire.calls.filter((m) => m === 'app.list').length - before).toBe(1)
    }
  })

  test('agents.changed re-reads the roster', async () => {
    const s = await online()
    s.wire.answers.set('agents.list', { agents: [] })
    s.wire.notify('agents.changed', { id: 'a1' })
    await settle()
    expect(s.host.agents).toEqual([])
  })
})

describe('answers of another shape are an error state, not a crash', () => {
  test('a malformed app.list keeps the apps it had and says why; the next good answer clears it', async () => {
    const s = await online()
    s.wire.answers.set('app.list', { apps: null })
    await s.host.refresh()
    expect(s.host.apps.map((a) => a.id)).toEqual(['calculator'])
    expect(s.host.appsError).toContain('app.list')
    s.wire.answers.set('app.list', { apps: [calculator, agents] })
    await s.host.refresh()
    expect(s.host.appsError).toBeNull()
    expect(s.host.apps.map((a) => a.id)).toEqual(['calculator', 'agents'])
  })

  test('{ agents: null } leaves the roster alone and sets agentsError', async () => {
    const s = await online()
    s.wire.answers.set('agents.list', { agents: null })
    s.wire.notify('agents.changed', {})
    await settle()
    expect(s.host.agents.map((a) => a.id)).toEqual(['a1'])
    expect(s.host.agentsError).toContain('agents.list')
  })

  test('a malformed catalog rejects with a message the page can show', async () => {
    const s = await online()
    s.wire.answers.set('app.available', { apps: [{ id: 1 }], errors: [] })
    expect(await s.host.catalog().catch((e: unknown) => e)).toBeInstanceOf(Error)
  })

  test('the registry problems of the catalog are deduplicated', async () => {
    const s = await online()
    s.wire.answers.set('app.available', { apps: [], errors: ['x', 'x', 'y'] })
    expect((await s.host.catalog()).errors).toEqual(['x', 'y'])
  })
})

describe('window.open requests', () => {
  test('reach the listeners with the window kind defaulting to main', async () => {
    const s = await online()
    const seen: unknown[] = []
    const off = s.host.onWindowRequest((r) => seen.push(r))
    s.wire.notify('window.open', { app: 'calculator' })
    s.wire.notify('window.open', { app: 'notes', window: 'editor', params: { id: 3 } })
    await settle()
    expect(seen).toEqual([
      { app: 'calculator', window: 'main' },
      { app: 'notes', window: 'editor', params: { id: 3 } }
    ])
    off()
    s.wire.notify('window.open', { app: 'calculator' })
    await settle()
    expect(seen).toHaveLength(2)
  })

  test('a request without an app id, or with params that are not an object, is ignored or stripped', async () => {
    const s = await online()
    const seen: unknown[] = []
    s.host.onWindowRequest((r) => seen.push(r))
    s.wire.notify('window.open', { window: 'main' })
    s.wire.notify('window.open', { app: 7 })
    s.wire.notify('window.open', { app: 'notes', params: [1, 2] })
    await settle()
    expect(seen).toEqual([{ app: 'notes', window: 'main' }])
  })
})

describe('the api before and after a connection', () => {
  test('a call with no connection rejects with "Not connected"', async () => {
    const s = setup()
    const error = await s.host.api.call('app.list').catch((e: unknown) => e)
    expect(error).toBeInstanceOf(HostError)
    expect((error as HostError).message).toBe('Not connected to vos')
  })

  test('a story can supply how a window’s view opens (it fails, or never answers) without a session', async () => {
    const s = setup()
    const api = { call: () => Promise.resolve({ apps: [] }), onNotification: () => () => undefined }
    s.host.useFake(api as never, [], {
      openView: () => Promise.reject(new Error('refused'))
    })
    await expect(
      s.host.openView({ app: 'calculator', window: 'main', windowId: 'w', size: { w: 1, h: 1 } })
    ).rejects.toThrow('refused')
    await expect(s.host.closeView({} as never)).resolves.toBeUndefined()
  })

  test('openView with no session rejects instead of throwing', async () => {
    const s = setup()
    await expect(
      s.host.openView({ app: 'calculator', window: 'main', windowId: 'w', size: { w: 1, h: 1 } })
    ).rejects.toThrow('Not connected')
  })
})
