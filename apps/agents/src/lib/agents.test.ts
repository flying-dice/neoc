import { describe, expect, test } from 'bun:test'
import { Agents } from './agents.svelte'
import { SCRIPTED, conversation, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend({ pace: 1 })): Promise<Agents> {
  const agents = new Agents(backend.rpc, backend.events)
  agents.listen()
  await agents.load()
  return agents
}

/** Wait until `done()` holds (the agent answers on timers of its own). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const said = (agents: Agents): string[] => agents.open?.messages.map((m) => `${m.role}: ${m.text}`) ?? []

describe('a window that has just opened', () => {
  test('lists the agents, has no chats and no chat open', async () => {
    const agents = await opened()
    expect(agents.agents).toEqual([SCRIPTED])
    expect(agents.chats).toEqual([])
    expect(agents.open).toBeNull()
    expect(agents.failure).toBeNull()
    expect(agents.sendable).toBe(false)
  })

  test('lists the chats that are already there, without opening one', async () => {
    const agents = await opened(createFakeBackend({ chats: [conversation('a', 'First', [['user', 'First']])] }))
    expect(agents.chats.map((c) => c.title)).toEqual(['First'])
    expect(agents.open).toBeNull()
  })
})

describe('a chat', () => {
  test('starting one opens it, empty, and lists it', async () => {
    const agents = await opened()
    expect(await agents.startChat(SCRIPTED)).toBe(true)
    expect(agents.open?.messages).toEqual([])
    expect(agents.open?.chat.title).toBe('New chat')
    expect(agents.chats).toHaveLength(1)
  })

  test('a message is sent, the answer comes a piece at a time, and the turn ends', async () => {
    const agents = await opened()
    await agents.startChat(SCRIPTED)
    agents.draft = 'hello'
    expect(agents.sendable).toBe(true)
    await agents.send()
    expect(agents.draft).toBe('')
    expect(agents.open?.busy).toBe(true)
    expect(agents.sendable).toBe(false)
    await until(() => agents.open?.busy === false)
    expect(said(agents)).toEqual(['user: hello', 'agent: Hi there'])
    expect(agents.open?.messages.every((m) => m.done)).toBe(true)
  })

  test('the first message names the chat, later ones do not', async () => {
    const agents = await opened()
    await agents.startChat(SCRIPTED)
    agents.draft = 'hello'
    await agents.send()
    await until(() => agents.open?.busy === false)
    agents.draft = 'and another thing'
    await agents.send()
    await until(() => agents.open?.busy === false)
    expect(agents.open?.chat.title).toBe('hello')
    expect(agents.chats.map((c) => c.title)).toEqual(['hello'])
    expect(said(agents).at(-1)).toBe('agent: echo: and another thing')
  })

  test('nothing is sent while the agent answers, and what is typed stays', async () => {
    const agents = await opened()
    await agents.startChat(SCRIPTED)
    agents.draft = 'slow'
    await agents.send()
    agents.draft = 'wait'
    await agents.send()
    expect(agents.draft).toBe('wait')
    expect(agents.open?.messages).toHaveLength(2)
  })

  test('stopping an answer ends the turn and keeps what was said', async () => {
    const agents = await opened()
    await agents.startChat(SCRIPTED)
    agents.draft = 'slow'
    await agents.send()
    await until(() => said(agents).at(-1) === 'agent: Thinking')
    await agents.stop()
    expect(agents.open?.busy).toBe(false)
    expect(said(agents)).toEqual(['user: slow', 'agent: Thinking'])
    expect(agents.open?.messages.at(-1)?.done).toBe(true)
  })

  test('opening another chat shows it; pieces of the one left behind are not added to it', async () => {
    const agents = await opened(createFakeBackend({ pace: 1, chats: [conversation('old', 'Old', [['user', 'Old'], ['agent', 'echo: Old']])] }))
    await agents.startChat(SCRIPTED)
    agents.draft = 'one two three'
    await agents.send()
    const old = agents.chats.find((c) => c.id === 'old')
    await agents.openChat(old!)
    await Bun.sleep(20)
    expect(said(agents)).toEqual(['user: Old', 'agent: echo: Old'])
  })

  test('back leaves the chat for the list', async () => {
    const agents = await opened()
    await agents.startChat(SCRIPTED)
    agents.back()
    expect(agents.open).toBeNull()
    expect(agents.chats).toHaveLength(1)
  })
})

describe('the host connection', () => {
  test('when it comes back, the open chat is read again', async () => {
    const backend = createFakeBackend({ pace: 1 })
    const agents = await opened(backend)
    await agents.startChat(SCRIPTED)
    // said through another window: this one heard nothing
    await backend.rpc.send({ chat: agents.open!.chat.id, text: 'hello' })
    await Bun.sleep(20)
    backend.emit('host:reconnected')
    await until(() => said(agents).length === 2)
    expect(said(agents)).toEqual(['user: hello', 'agent: Hi there'])
  })

  test('after `listen` is stopped, events are not heard', async () => {
    const backend = createFakeBackend({ pace: 1 })
    const agents = new Agents(backend.rpc, backend.events)
    const stop = agents.listen()
    await agents.load()
    await agents.startChat(SCRIPTED)
    stop()
    agents.draft = 'hello'
    await agents.send()
    await Bun.sleep(20)
    expect(said(agents)).toEqual(['user: hello', 'agent: '])
  })
})

describe('a backend that cannot be reached', () => {
  test('loading says why and shows nothing', async () => {
    const agents = await opened(createFailingBackend('The host is not answering'))
    expect(agents.failure).toBe('The host is not answering')
    expect(agents.agents).toEqual([])
  })

  test('a message that could not be sent is back in the field, with the reason', async () => {
    const backend = createFakeBackend({ pace: 1 })
    const agents = await opened(backend)
    await agents.startChat(SCRIPTED)
    const failing = new Agents({ ...backend.rpc, send: () => Promise.reject(new Error('Connection lost')) }, backend.events)
    await failing.load()
    await failing.openChat(agents.chats[0]!)
    failing.draft = 'hello'
    await failing.send()
    expect(failing.draft).toBe('hello')
    expect(failing.failure).toBe('Connection lost')
    expect(failing.open?.messages).toEqual([])
  })

  test('a rejection that is not an Error is shown as text', async () => {
    const backend = createFakeBackend()
    const agents = new Agents({ ...backend.rpc, agents: () => Promise.reject('offline') }, backend.events)
    await agents.load()
    expect(agents.failure).toBe('offline')
  })
})
