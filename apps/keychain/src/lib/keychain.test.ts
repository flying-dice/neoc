import { describe, expect, test } from 'bun:test'
import { NO_KEYCHAIN, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'
import { itemsSummary } from './format'
import { Keychain } from './keychain.svelte'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Keychain> {
  const keychain = new Keychain(backend.rpc, backend.events)
  keychain.listen()
  await keychain.load()
  return keychain
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const names = (keychain: Keychain): string[] => keychain.items.map((item) => `${item.service} ${item.account}`)

describe('a window that has just opened', () => {
  test('says nothing about the keychain until the first answer', () => {
    const backend = createFakeBackend()
    const keychain = new Keychain(backend.rpc, backend.events)
    expect(keychain.loaded).toBe(false)
    expect(keychain.items).toEqual([])
  })

  test('shows the items by service, and reads no password', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    expect(keychain.store).toEqual({ available: true, problem: '' })
    expect(names(keychain)).toEqual(['backup.example.com ada', 'ghcr.io ada-lovelace', 'router.home admin'])
    expect(keychain.items.every((item) => keychain.password(item) === null)).toBe(true)
    expect(backend.reads()).toBe(0)
    expect(keychain.failure).toBeNull()
  })

  test('says why when the machine has no keychain: an answer, not a failed call', async () => {
    const keychain = await opened(createFakeBackend({ problem: NO_KEYCHAIN }))
    expect(keychain.store).toEqual({ available: false, problem: NO_KEYCHAIN })
    expect(keychain.failure).toBeNull()
  })

  test('keeps the reason when the backend itself cannot be reached', async () => {
    const keychain = await opened(createFailingBackend('The host is not answering.'))
    expect(keychain.failure).toBe('The host is not answering.')
    expect(keychain.loaded).toBe(false)
  })
})

describe('adding an item', () => {
  test('keeps the password in the keychain, shows the item and empties the fields', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    keychain.edit({ service: ' nas.home ', account: ' ada ', password: ' kept as typed ' })
    expect(keychain.ready).toBe(true)
    expect(await keychain.add()).toBe(true)
    expect(names(keychain)).toContain('nas.home ada')
    expect(backend.kept('nas.home', 'ada')).toBe(' kept as typed ')
    expect(keychain.draft).toEqual({ service: '', account: '', password: '' })
  })

  test('keeps what is typed while the keychain is asked: it is the next item', async () => {
    const keychain = await opened(createFakeBackend({ pace: 20 }))
    keychain.edit({ service: 'nas.home', account: 'ada', password: 'first' })
    const adding = keychain.add()
    keychain.edit({ service: 'tv.home', account: 'ada', password: 'second' })
    expect(await adding).toBe(true)
    expect(names(keychain)).toContain('nas.home ada')
    expect(keychain.draft).toEqual({ service: 'tv.home', account: 'ada', password: 'second' })
  })

  test('is not sent without a service or an account', async () => {
    const keychain = await opened()
    keychain.edit({ service: 'nas.home', account: '  ', password: 'secret' })
    expect(keychain.ready).toBe(false)
    expect(await keychain.add()).toBe(false)
    expect(keychain.items).toHaveLength(3)
    expect(keychain.refused).toBeNull()
  })

  test('without a password shows an item the keychain has already, and changes nothing there', async () => {
    const backend = createFakeBackend()
    backend.outside('github.com', 'ada', 'made by git')
    const keychain = await opened(backend)
    keychain.edit({ service: 'github.com', account: 'ada' })
    expect(await keychain.add()).toBe(true)
    expect(names(keychain)).toContain('github.com ada')
    expect(backend.kept('github.com', 'ada')).toBe('made by git')
  })

  test('does not replace a password another program keeps: the form says so, the window does not', async () => {
    const backend = createFakeBackend()
    backend.outside('github.com', 'ada', 'made by git')
    const keychain = await opened(backend)
    keychain.edit({ service: 'github.com', account: 'ada', password: 'a slip' })
    expect(await keychain.add()).toBe(false)
    expect(keychain.refused).toBe('The keychain has a password for this service and account already. Leave the password empty to show it here.')
    expect(keychain.failure).toBeNull()
    expect(backend.kept('github.com', 'ada')).toBe('made by git')
    // what was typed is still there to be put right
    expect(keychain.draft.service).toBe('github.com')
  })

  test('without a password is refused when the keychain has none, and the refusal goes when the item is typed again', async () => {
    const keychain = await opened()
    keychain.edit({ service: 'nowhere.example', account: 'ada' })
    expect(await keychain.add()).toBe(false)
    expect(keychain.refused).toBe('The keychain has no password for this service and account. Type one to keep it.')
    keychain.edit({ password: 's' })
    expect(keychain.refused).toBeNull()
  })

  test('that failed to get through is the window to say, not the form', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    const away = new Keychain({ ...backend.rpc, add: () => Promise.reject(new Error('The host is not answering.')) }, backend.events)
    await away.load()
    away.edit({ service: 'nas.home', account: 'ada', password: 'secret' })
    expect(await away.add()).toBe(false)
    expect(away.failure).toBe('The host is not answering.')
    expect(away.refused).toBeNull()
    expect(keychain.failure).toBeNull()
  })
})

describe('a password', () => {
  test('is read when it is asked for, and forgotten when it is hidden', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    const router = keychain.items[2]
    expect(await keychain.reveal(router)).toBe(true)
    expect(keychain.password(router)).toBe('tr0ub4dor&3')
    expect(keychain.password(keychain.items[0])).toBeNull()
    expect(backend.reads()).toBe(1)
    keychain.hide(router)
    expect(keychain.password(router)).toBeNull()
  })

  test('marks its item busy while the keychain is asked', async () => {
    const keychain = await opened(createFakeBackend({ pace: 20 }))
    const router = keychain.items[2]
    const asked = keychain.reveal(router)
    expect(keychain.busy(router)).toBe(true)
    expect(keychain.busy(keychain.items[0])).toBe(false)
    await asked
    expect(keychain.busy(router)).toBe(false)
  })

  test('that the keychain no longer has is said at its item, until it is asked for again', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    const router = keychain.items[2]
    backend.lost('router.home', 'admin')
    expect(await keychain.reveal(router)).toBe(false)
    expect(keychain.problem(router)).toBe('The keychain no longer has this item.')
    expect(keychain.failure).toBeNull()
    backend.outside('router.home', 'admin', 'back again')
    expect(await keychain.reveal(router)).toBe(true)
    expect(keychain.problem(router)).toBeNull()
  })
})

describe('deleting an item', () => {
  test('asks first, and Cancel keeps it', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    const router = keychain.items[2]
    keychain.ask(router)
    expect(keychain.asking).toEqual(router)
    keychain.keep()
    expect(keychain.asking).toBeNull()
    expect(await keychain.remove()).toBe(false)
    expect(backend.kept('router.home', 'admin')).toBe('tr0ub4dor&3')
  })

  test('takes it from the keychain and from the list, with what was shown of it', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    const router = keychain.items[2]
    await keychain.reveal(router)
    keychain.ask(router)
    expect(await keychain.remove()).toBe(true)
    expect(keychain.asking).toBeNull()
    expect(names(keychain)).toEqual(['backup.example.com ada', 'ghcr.io ada-lovelace'])
    expect(backend.kept('router.home', 'admin')).toBeUndefined()
    expect(keychain.password(router)).toBeNull()
  })
})

describe('a change made elsewhere', () => {
  test('by an agent reaches the window', async () => {
    const backend = createFakeBackend()
    const keychain = await opened(backend)
    backend.elsewhere('nas.home', 'agent', 'made by an agent')
    await until(() => keychain.items.length === 4)
    expect(names(keychain)).toContain('nas.home agent')
  })

  test('is read again when the host connection comes back, and not after the window stopped listening', async () => {
    const backend = createFakeBackend()
    const keychain = new Keychain(backend.rpc, backend.events)
    const stop = keychain.listen()
    backend.emit('host:reconnected')
    await until(() => keychain.loaded)
    stop()
    backend.elsewhere('nas.home', 'agent', 'unheard')
    await Bun.sleep(10)
    expect(keychain.items).toHaveLength(3)
  })
})

describe('the summary', () => {
  test('counts the items in words', () => {
    expect(itemsSummary(0)).toBe('No items')
    expect(itemsSummary(1)).toBe('1 item')
    expect(itemsSummary(3)).toBe('3 items')
  })
})
