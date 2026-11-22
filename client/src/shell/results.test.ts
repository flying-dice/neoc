import { describe, expect, test } from 'bun:test'
import { appListSchema, callParsed, catalogSchema, deviceListSchema, ResultError } from './results'
import type { HostApi } from './hostApi'

const apiAnswering = (result: unknown): HostApi => ({
  call: () => Promise.resolve(result),
  onNotification: () => () => undefined
})

const app = {
  id: 'notes',
  name: 'Notes',
  version: '1.0.0',
  state: 'running',
  source: 'file',
  permissions: []
}

describe('callParsed', () => {
  test('returns the parsed result', async () => {
    const list = await callParsed(apiAnswering({ apps: [app] }), 'app.list', {}, appListSchema)
    expect(list.apps[0].id).toBe('notes')
  })

  test('a result of another shape rejects with a ResultError naming the method and the field', async () => {
    const error = await callParsed(
      apiAnswering({ apps: null }),
      'app.list',
      {},
      appListSchema
    ).catch((e: unknown) => e)
    expect(error).toBeInstanceOf(ResultError)
    expect((error as ResultError).message).toContain('app.list')
    expect((error as ResultError).message).toContain('apps')
  })

  test.each([null, 'ok', 7, [], { devices: [{ id: 1 }] }])(
    'devices.list rejects %p',
    async (bad) => {
      const error = await callParsed(
        apiAnswering(bad),
        'host.devices.list',
        {},
        deviceListSchema
      ).catch((e: unknown) => e)
      expect(error).toBeInstanceOf(ResultError)
    }
  )

  test('an app in an unknown state is refused rather than shown half-working', async () => {
    const error = await callParsed(
      apiAnswering({ apps: [{ ...app, state: 'exploded' }] }),
      'app.list',
      {},
      appListSchema
    ).catch((e: unknown) => e)
    expect(error).toBeInstanceOf(ResultError)
  })

  test('fields the shell does not read are dropped', async () => {
    const { apps } = await callParsed(
      apiAnswering({ apps: [{ ...app, secret: { TOKEN: 'secret' } }] }),
      'app.list',
      {},
      appListSchema
    )
    expect(apps[0] as unknown).toEqual(app)
  })
})

describe('the catalog', () => {
  test('registry problems are deduplicated at the edge (the page keys them by text)', () => {
    const catalog = catalogSchema.parse({ apps: [], errors: ['bad', 'bad', 'worse', 'bad'] })
    expect(catalog.errors).toEqual(['bad', 'worse'])
  })

  test('a catalog entry carries the digest the install is pinned to, and a malformed one is refused', () => {
    const entry = {
      url: 'file:///r/notes/app-ref.json',
      id: 'notes',
      name: 'Notes',
      version: '1.0.0',
      source: 'this computer',
      sha256: 'ab'.repeat(32)
    }
    expect(catalogSchema.parse({ apps: [entry], errors: [] }).apps[0].sha256).toBe('ab'.repeat(32))
    expect(() =>
      catalogSchema.parse({ apps: [{ ...entry, sha256: undefined }], errors: [] })
    ).toThrow()
    expect(() =>
      catalogSchema.parse({ apps: [{ ...entry, sha256: 'nope' }], errors: [] })
    ).toThrow()
  })
})
