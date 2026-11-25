import { describe, expect, test } from 'bun:test'
import { Example, HELLO } from './example.svelte'
import { PARTS, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Example> {
  const example = new Example(backend.rpc, backend.events)
  example.listen()
  await example.load()
  return example
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

describe('a window that has just opened', () => {
  test('shows nothing until the backend answers', () => {
    const backend = createFakeBackend()
    const example = new Example(backend.rpc, backend.events)
    expect(example.loaded).toBe(false)
    expect(example.parts).toEqual([])
  })

  test('lists the parts the backend serves, and looks at none of them', async () => {
    const example = await opened()
    expect(example.loaded).toBe(true)
    expect(example.parts.map((p) => p.id)).toEqual(['contract', 'backend', 'page'])
    expect(example.selected).toBeUndefined()
    expect(example.failure).toBeNull()
  })

  test('asks again when the host comes back', async () => {
    const backend = createFakeBackend()
    await opened(backend)
    backend.emit('host:reconnected')
    await until(() => backend.calls.filter((c) => c === 'parts').length === 2)
  })

  test('says why when the host cannot be reached, and stays unloaded', async () => {
    const example = await opened(createFailingBackend('The host is not reachable.'))
    expect(example.loaded).toBe(false)
    expect(example.failure).toBe('The host is not reachable.')
  })
})

describe('picking a part', () => {
  test('looks at it, and at nothing once let go', async () => {
    const example = await opened()
    example.select(PARTS[1].id)
    expect(example.selected?.title).toBe('Backend')
    expect(example.saying).toBe(false)
    example.select(null)
    expect(example.selected).toBeUndefined()
  })

  test('the greeting is a row of its own, and no part', async () => {
    const example = await opened()
    example.select(HELLO)
    expect(example.saying).toBe(true)
    expect(example.selected).toBeUndefined()
  })
})

describe('greeting someone', () => {
  test('cannot be sent without a name', async () => {
    const backend = createFakeBackend()
    const example = await opened(backend)
    expect(example.canGreet).toBe(false)
    example.type('   ')
    expect(example.canGreet).toBe(false)
    await example.greet()
    expect(backend.calls).toEqual(['parts'])
  })

  test('sends the name and keeps the backend’s answer', async () => {
    const example = await opened()
    example.type('  Ada ')
    expect(example.canGreet).toBe(true)
    await example.greet()
    expect(example.greeting).toBe("Hello, Ada! This came from the Example app's Luau backend.")
    expect(example.sending).toBe(false)
  })

  test('is not sent twice while one is in flight', async () => {
    const backend = createFakeBackend({ pace: 5 })
    const example = await opened(backend)
    example.type('Ada')
    const first = example.greet()
    expect(example.sending).toBe(true)
    expect(example.canGreet).toBe(false)
    await example.greet()
    await first
    expect(backend.calls.filter((c) => c.startsWith('greet'))).toEqual(['greet Ada'])
  })

  test('a refusal is said at the field, not the window, and goes when the name is typed again', async () => {
    const backend = createFakeBackend()
    // the backend has the last word: here it refuses a name the client let through
    const refusing = new Example({ ...backend.rpc, greet: () => Promise.reject(Object.assign(new Error('Type a name to greet.'), { code: -32010 })) }, backend.events)
    await refusing.load()
    refusing.type('x')
    await refusing.greet()
    expect(refusing.refusal).toBe('Type a name to greet.')
    expect(refusing.failure).toBeNull()
    refusing.type('xy')
    expect(refusing.refusal).toBeNull()
  })

  test('a host failure goes to the window', async () => {
    const backend = createFakeBackend()
    const example = new Example({ ...backend.rpc, greet: () => Promise.reject(new Error('The host is not reachable.')) }, backend.events)
    await example.load()
    example.type('Ada')
    await example.greet()
    expect(example.failure).toBe('The host is not reachable.')
    expect(example.refusal).toBeNull()
  })
})
