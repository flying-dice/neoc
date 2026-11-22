import { beforeEach, describe, expect, test } from 'bun:test'
import { Devices } from './devices.svelte'
import { host } from './host.svelte'
import { createFakeHost } from '../shell/fakeHost'
import type { HostApi } from '../shell/hostApi'

let fake: ReturnType<typeof createFakeHost>
let devices: Devices
/** What the host answers instead of the fake, per method, while a test says so. */
let failing: Record<string, unknown>

beforeEach(() => {
  fake = createFakeHost()
  failing = {}
  const api: HostApi = {
    onNotification: (listener) => fake.onNotification(listener),
    call: (method, params) =>
      method in failing ? Promise.reject(failing[method]) : fake.call(method, params)
  }
  host.useFake(api)
  devices = new Devices()
})

const names = (): string[] => devices.list.map((d) => d.name)

describe('the list', () => {
  test('is what the host lists, with this device marked', async () => {
    expect(devices.loaded).toBe(false)
    await devices.load()
    expect(devices.loaded).toBe(true)
    expect(names()).toEqual(['Safari on iPad', 'Chrome on Mac', 'Firefox on Linux'])
    expect(devices.list.filter((d) => d.current).map((d) => d.id)).toEqual(['d1'])
    expect(devices.error).toBeNull()
  })

  test('that could not be read says why, and is not taken for an empty one', async () => {
    failing['host.devices.list'] = new Error('Not connected to vos')
    await devices.load()
    expect(devices.error).toBe('The paired devices could not be read: Not connected to vos')
    expect(devices.loaded).toBe(false)
  })

  test('that could not be read again keeps the last one, and a read that works clears the problem', async () => {
    await devices.load()
    failing['host.devices.list'] = new Error('Not connected to vos')
    await devices.load()
    expect(names()).toHaveLength(3)
    expect(devices.loaded).toBe(true)
    expect(devices.error).toContain('Not connected to vos')
    failing = {}
    await devices.load()
    expect(devices.error).toBeNull()
  })

  test('in a shape the shell does not know is a problem, not a list', async () => {
    fake.devices = [{ id: 'd9', name: 'Old host' }] as never
    await devices.load()
    expect(devices.loaded).toBe(false)
    expect(devices.error).toContain('host.devices.list')
  })
})

describe('revoking', () => {
  test('takes the device out of the list', async () => {
    await devices.load()
    await devices.revoke(devices.list[1]!)
    expect(names()).toEqual(['Safari on iPad', 'Firefox on Linux'])
    expect(devices.error).toBeNull()
  })

  test('that the host refuses says which device and why, and the device is still listed', async () => {
    await devices.load()
    failing['host.devices.revoke'] = new Error('not found: d2')
    await devices.revoke(devices.list[1]!)
    expect(devices.error).toBe('Chrome on Mac could not be revoked: not found: d2')
    expect(names()).toHaveLength(3)
  })

  test('a device that another client revoked meanwhile is gone from the list either way', async () => {
    await devices.load()
    const gone = devices.list[1]!
    fake.devices = fake.devices.filter((d) => d.id !== gone.id)
    failing['host.devices.revoke'] = new Error('not found: d2')
    await devices.revoke(gone)
    expect(names()).toEqual(['Safari on iPad', 'Firefox on Linux'])
    expect(devices.error).toContain('could not be revoked')
  })
})
