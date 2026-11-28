import { describe, expect, test } from 'bun:test'
import { Agents } from './agents.svelte'
import {
  CODER,
  ECHO,
  IMAGES,
  NO_KEYCHAIN,
  NO_SKILLS,
  PICTURE,
  PICTURED,
  SCRIPTED,
  SECRETS,
  SKILLS,
  UNREACHABLE,
  conversation,
  createFailingBackend,
  createFakeBackend,
  type FakeBackend
} from './fakeApi'

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

const said = (agents: Agents): string[] => agents.open?.items.map((i) => `${i.kind}: ${i.text}`) ?? []

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
  test('starting one opens it, empty, and it is made, and listed, only when something is said in it', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    expect(agents.open?.items).toEqual([])
    expect(agents.open?.chat.title).toBe('New chat')
    expect(agents.chats).toHaveLength(0)
    // left unsaid, it leaves nothing behind
    agents.back()
    agents.startChat(SCRIPTED)
    await agents.removeChat()
    expect(agents.open).toBeNull()
    expect(agents.chats).toHaveLength(0)
    // the first message makes it
    agents.startChat(SCRIPTED)
    agents.draft = 'hello'
    await agents.send()
    expect(agents.chats).toHaveLength(1)
    expect(agents.open?.chat.id).toBe(agents.chats[0]?.id)
    expect(agents.open?.chat.title).toBe('hello')
  })

  test('a message is sent, the answer comes a piece at a time, and the turn ends', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'hello'
    expect(agents.sendable).toBe(true)
    await agents.send()
    expect(agents.draft).toBe('')
    expect(agents.open?.busy).toBe(true)
    expect(agents.sendable).toBe(false)
    await until(() => agents.open?.busy === false)
    expect(said(agents)).toEqual(['user: hello', 'assistant: Hi there'])
    expect(agents.open?.items.every((i) => i.done)).toBe(true)
  })

  test('the first message names the chat, later ones do not', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'hello'
    await agents.send()
    await until(() => agents.open?.busy === false)
    agents.draft = 'and another thing'
    await agents.send()
    await until(() => agents.open?.busy === false)
    expect(agents.open?.chat.title).toBe('hello')
    expect(agents.chats.map((c) => c.title)).toEqual(['hello'])
    expect(said(agents).at(-1)).toBe('assistant: echo: and another thing')
  })

  test('nothing is sent while the agent answers, and what is typed stays', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'slow'
    await agents.send()
    agents.draft = 'wait'
    await agents.send()
    expect(agents.draft).toBe('wait')
    expect(said(agents).filter((line) => line.startsWith('user'))).toEqual(['user: slow'])
  })

  test('stopping an answer ends the turn and keeps what was said', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'slow'
    await agents.send()
    await until(() => said(agents).at(-1) === 'assistant: Thinking')
    await agents.stop()
    expect(agents.open?.busy).toBe(false)
    expect(said(agents)).toEqual(['user: slow', 'assistant: Thinking'])
    expect(agents.open?.items.at(-1)?.done).toBe(true)
  })

  test('opening another chat shows it; pieces of the one left behind are not added to it', async () => {
    const agents = await opened(createFakeBackend({ pace: 1, chats: [conversation('old', 'Old', [['user', 'Old'], ['assistant', 'echo: Old']])] }))
    agents.startChat(SCRIPTED)
    agents.draft = 'one two three'
    await agents.send()
    const old = agents.chats.find((c) => c.id === 'old')
    await agents.openChat(old!)
    await Bun.sleep(20)
    expect(said(agents)).toEqual(['user: Old', 'assistant: echo: Old'])
  })

  test('back leaves the chat for the list', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'hello'
    await agents.send()
    agents.back()
    expect(agents.open).toBeNull()
    expect(agents.chats).toHaveLength(1)
  })
})

describe('what an agent does besides talking', () => {
  async function after(text: string): Promise<Agents> {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = text
    await agents.send()
    await until(() => agents.open?.busy === false)
    return agents
  }

  test('what it thought, its plan and a tool call that changed a file are in the chat', async () => {
    const agents = await after('tool')
    expect(agents.open?.items.map((i) => i.kind)).toEqual(['user', 'reasoning', 'plan', 'tool', 'assistant'])
    expect(agents.open?.items[3]).toMatchObject({ name: 'Add milk to list.md' })
  })

  test('a permission it asks for holds the turn until it is answered, and the agent goes on with the answer', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'ask'
    await agents.send()
    await until(() => agents.open?.waiting === true)
    expect(agents.open?.busy).toBe(true)
    agents.draft = 'never mind'
    expect(agents.sendable).toBe(false)
    await agents.answer(agents.open!.items.at(-1)!, 'allow')
    await until(() => agents.open?.busy === false)
    expect(agents.open?.waiting).toBe(false)
    expect(said(agents).at(-1)).toBe('assistant: Added.')
    expect(agents.open?.items.find((i) => i.kind === 'permission')?.data).toContain('"chosen":"allow"')
  })

  test('a permission can be refused', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'ask'
    await agents.send()
    await until(() => agents.open?.waiting === true)
    await agents.answer(agents.open!.items.at(-1)!, 'reject')
    await until(() => agents.open?.busy === false)
    expect(said(agents).at(-1)).toBe('assistant: Then I will not.')
  })

  test('an answer the agent did not offer is refused, with the reason', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'ask'
    await agents.send()
    await until(() => agents.open?.waiting === true)
    await agents.answer(agents.open!.items.at(-1)!, 'whatever')
    expect(agents.failure).toBe('The agent did not offer that answer.')
    expect(agents.open?.waiting).toBe(true)
  })

  test('stopping a turn that waits for an answer leaves the question unanswered', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'ask'
    await agents.send()
    await until(() => agents.open?.waiting === true)
    await agents.stop()
    expect(agents.open?.busy).toBe(false)
    expect(agents.open?.waiting).toBe(false)
    expect(agents.open?.items.at(-1)).toMatchObject({ kind: 'permission', done: true })
  })

  test('deleting the open chat closes it and takes it off the list', async () => {
    const agents = await after('hello')
    await agents.removeChat()
    expect(agents.open).toBeNull()
    expect(agents.chats).toEqual([])
  })
})

describe('the agents', () => {
  test('one is added with a name, an address, a folder and a token, and the form closes', async () => {
    const agents = await opened(createFakeBackend({ agents: [] }))
    agents.edit(null)
    expect(agents.savable).toBe(false)
    expect(agents.form?.folder).toBe('/')
    Object.assign(agents.form!, { name: 'Ada', url: 'ws://example.com:5424/acp', folder: '/home/ada', token: 'secret' })
    expect(agents.savable).toBe(true)
    await agents.save()
    expect(agents.form).toBeNull()
    expect(agents.agents).toMatchObject([{ name: 'Ada', url: 'ws://example.com:5424/acp', folder: '/home/ada', token: true }])
  })

  test('a form the backend refuses stays open, with the reason', async () => {
    const agents = await opened()
    agents.edit(null)
    Object.assign(agents.form!, { name: 'Ada', url: 'https://example.com/agent' })
    await agents.save()
    expect(agents.form).not.toBeNull()
    expect(agents.failure).toBe('The address must be a ws:// or wss:// URL, like ws://example.com:5424/acp.')
    Object.assign(agents.form!, { url: 'ws://example.com/acp', folder: 'project' })
    await agents.save()
    expect(agents.failure).toBe("The folder must be an absolute path on the agent's machine, like /home/me/project.")
  })

  test('one is changed, and its chats are listed under its new name', async () => {
    const agents = await opened()
    agents.startChat(SCRIPTED)
    agents.draft = 'hello'
    await agents.send()
    agents.edit(agents.agents[0]!)
    expect(agents.open).not.toBeNull()
    expect(agents.form?.folder).toBe(SCRIPTED.folder)
    agents.form!.name = 'Renamed'
    await agents.save()
    expect(agents.agents[0]?.name).toBe('Renamed')
    expect(agents.chats[0]?.agent).toBe('Renamed')
  })

  test('one is removed', async () => {
    const agents = await opened()
    agents.edit(agents.agents[0]!)
    await agents.remove()
    expect(agents.agents).toEqual([])
    expect(agents.form).toBeNull()
  })
})

describe('the types of agent', () => {
  test('adding one starts with its type; the form for that type opens, and Cancel leaves the picker', async () => {
    const agents = await opened()
    agents.add()
    expect(agents.pane).toBe('types')
    expect(agents.form).toBeNull()
    agents.back()
    expect(agents.pane).toBeNull()
    agents.add()
    await agents.edit(null, 'program')
    expect(agents.picking).toBe(false)
    expect(agents.pane).toBe('agent')
    expect(agents.form).toMatchObject({ kind: 'program', folder: '' })
  })

  test('a program agent is added with its command and a folder of this machine', async () => {
    const agents = await opened(createFakeBackend({ agents: [] }))
    await agents.edit(null, 'program')
    Object.assign(agents.form!, { name: 'Echo', command: 'python3 -u echo.py' })
    expect(agents.savable).toBe(false)
    agents.form!.folder = '/nowhere'
    await agents.save()
    expect(agents.failure).toBe('There is no folder /nowhere on this machine.')
    agents.form!.folder = '/home/ada'
    await agents.save()
    expect(agents.form).toBeNull()
    expect(agents.agents).toMatchObject([{ name: 'Echo', kind: 'program', command: ['python3', '-u', 'echo.py'], folder: '/home/ada', url: '', token: false }])
  })

  test("a container agent's form offers Docker's images, the Skills app's skills and the keychain's items", async () => {
    const agents = await opened(createFakeBackend({ agents: [] }))
    await agents.edit(null, 'container')
    expect(agents.images).toEqual({ items: IMAGES, problem: null })
    expect(agents.skills).toEqual({ items: SKILLS, problem: null })
    expect(agents.secrets).toEqual({ items: SECRETS, problem: null })
    Object.assign(agents.form!, { name: 'Coder', image: IMAGES[0], command: 'claude-code-acp', skills: ['review'] })
    await agents.save()
    expect(agents.agents).toMatchObject([{ kind: 'container', image: IMAGES[0], folder: '/workspace', skills: ['review'], skills_dir: '/root/.claude/skills' }])
  })

  test('without the Skills app or the Keychain app, the form says why and still takes what is typed', async () => {
    const agents = await opened(createFakeBackend({ agents: [], skills: null, secrets: null }))
    await agents.edit(null, 'container')
    expect(agents.skills).toEqual({ items: [], problem: NO_SKILLS })
    expect(agents.secrets).toEqual({ items: [], problem: NO_KEYCHAIN })
    expect(agents.failure).toBeNull()
    Object.assign(agents.form!, { name: 'Coder', image: 'mine:1', command: 'agent' })
    await agents.save()
    expect(agents.agents).toMatchObject([{ image: 'mine:1' }])
  })

  test("a container agent's form and chats need Docker; the other types never do", async () => {
    const agents = await opened(createFakeBackend({ agents: [CODER, ECHO, SCRIPTED], engine: UNREACHABLE }))
    expect(agents.engine?.reachable).toBe(false)
    agents.startChat(SCRIPTED)
    expect(agents.dockerDown).toBe(false)
    agents.startChat(ECHO)
    expect(agents.dockerDown).toBe(false)
    agents.startChat(CODER)
    expect(agents.dockerDown).toBe(true)
    await agents.edit(null, 'container')
    expect(agents.dockerDown).toBe(true)
    expect(agents.images.problem).toBe(UNREACHABLE.problem)
    // another Docker, which can be reached: the images are read again
    agents.hostDraft = 'tcp://docker:2375'
    await agents.connect()
    expect(agents.dockerDown).toBe(false)
    expect(agents.engine?.host).toBe('tcp://docker:2375')
    expect(agents.images.items).toEqual(IMAGES)
  })

  test('Docker being away is never shown to one with no container agent open: no agents, the type picker, a remote or a program form', async () => {
    const agents = await opened(createFakeBackend({ agents: [], engine: UNREACHABLE }))
    expect(agents.engine?.reachable).toBe(false)
    expect(agents.dockerDown).toBe(false)
    agents.add()
    expect(agents.pane).toBe('types')
    expect(agents.dockerDown).toBe(false)
    await agents.edit(null, 'remote')
    expect(agents.dockerDown).toBe(false)
    await agents.edit(null, 'program')
    expect(agents.dockerDown).toBe(false)
    // a container agent's form needs it, and leaving it for the list does not
    await agents.edit(null, 'container')
    expect(agents.dockerDown).toBe(true)
    agents.back()
    expect(agents.dockerDown).toBe(false)
  })

  test("removing a container agent takes its chats with it; a program agent's chats stay", async () => {
    const agents = await opened(
      createFakeBackend({
        agents: [CODER, ECHO],
        chats: [conversation('c', 'In a container', [['user', 'hi']], CODER), conversation('p', 'A program', [['user', 'hi']], ECHO)]
      })
    )
    await agents.edit(agents.agentOf(CODER.id))
    await agents.remove()
    expect(agents.chats.map((c) => c.id)).toEqual(['p'])
    await agents.edit(agents.agentOf(ECHO.id))
    await agents.remove()
    expect(agents.agents).toEqual([])
    expect(agents.chats.map((c) => c.id)).toEqual(['p'])
  })

  test('the type stays: a change sends what the agent is, and nothing of another type', async () => {
    const backend = createFakeBackend({ agents: [ECHO] })
    const sent: VosApp.Agents.UpdateAgentParams[] = []
    const { updateAgent } = backend.rpc
    backend.rpc.updateAgent = (params) => (sent.push(params), updateAgent(params))
    const agents = await opened(backend)
    await agents.edit(agents.agents[0]!, 'remote')
    expect(agents.form?.kind).toBe('program')
    agents.form!.name = 'Echo two'
    await agents.save()
    expect(sent[0]).not.toHaveProperty('url')
    expect(agents.agents[0]).toMatchObject({ name: 'Echo two', kind: 'program' })
  })
})

describe("an agent's picture", () => {
  /** A backend whose calls to add and change an agent are kept, to see what was sent. */
  function recording(options: Parameters<typeof createFakeBackend>[0] = {}): { backend: FakeBackend; sent: Record<string, unknown>[] } {
    const backend = createFakeBackend(options)
    const sent: Record<string, unknown>[] = []
    const { addAgent, updateAgent } = backend.rpc
    backend.rpc.addAgent = (params) => (sent.push({ ...params }), addAgent(params))
    backend.rpc.updateAgent = (params) => (sent.push({ ...params }), updateAgent(params))
    return { backend, sent }
  }

  test('one is added with its picture', async () => {
    const agents = await opened(createFakeBackend({ agents: [] }))
    agents.edit(null)
    expect(agents.form?.avatar).toBe('')
    Object.assign(agents.form!, { name: 'Ada', url: 'ws://example.com/acp', avatar: PICTURE })
    await agents.save()
    expect(agents.agents).toMatchObject([{ name: 'Ada', avatar: PICTURE }])
  })

  test('one added without a picture has none, and none is sent', async () => {
    const { backend, sent } = recording({ agents: [] })
    const agents = await opened(backend)
    agents.edit(null)
    Object.assign(agents.form!, { name: 'Ada', url: 'ws://example.com/acp' })
    await agents.save()
    expect(sent[0]).not.toHaveProperty('avatar')
    expect(agents.agents[0]?.avatar).toBe('')
  })

  test('the form starts with the picture the agent has, and a change that leaves it does not send it again', async () => {
    const { backend, sent } = recording({ agents: [PICTURED] })
    const agents = await opened(backend)
    agents.edit(agents.agents[0]!)
    expect(agents.form?.avatar).toBe(PICTURE)
    agents.form!.name = 'Renamed'
    await agents.save()
    expect(sent[0]).not.toHaveProperty('avatar')
    expect(sent[0]).not.toHaveProperty('remove_avatar')
    expect(agents.agents[0]).toMatchObject({ name: 'Renamed', avatar: PICTURE })
  })

  test('a new picture replaces the one it has', async () => {
    const agents = await opened(createFakeBackend({ agents: [PICTURED] }))
    const other = PICTURE.replace('png', 'webp')
    agents.edit(agents.agents[0]!)
    agents.form!.avatar = other
    await agents.save()
    expect(agents.agents[0]?.avatar).toBe(other)
  })

  test('a picture removed is removed', async () => {
    const { backend, sent } = recording({ agents: [PICTURED] })
    const agents = await opened(backend)
    agents.edit(agents.agents[0]!)
    agents.form!.avatar = ''
    await agents.save()
    expect(sent[0]).toMatchObject({ remove_avatar: true })
    expect(agents.agents[0]?.avatar).toBe('')
  })

  test('a picture that is not a PNG, JPEG or WebP is refused, and the form stays', async () => {
    const agents = await opened()
    agents.edit(agents.agents[0]!)
    agents.form!.avatar = 'data:image/svg+xml;base64,PHN2Zz48L3N2Zz4='
    await agents.save()
    expect(agents.form).not.toBeNull()
    expect(agents.failure).toBe('The picture must be a PNG, JPEG or WebP image.')
  })

  test('a chat shows the picture of its agent, and none once the agent is removed', async () => {
    const agents = await opened(createFakeBackend({ agents: [PICTURED, SCRIPTED] }))
    agents.startChat(PICTURED)
    agents.draft = 'hello'
    await agents.send()
    const chat = agents.chats[0]!
    expect(chat.agent_id).toBe(PICTURED.id)
    expect(agents.avatarOf(chat)).toBe(PICTURE)
    agents.edit(agents.agents.find((a) => a.id === PICTURED.id)!)
    await agents.remove()
    expect(agents.avatarOf(chat)).toBe('')
  })
})

describe('an item too large to be sent', () => {
  /** A window with an open chat whose one item the backend has a longer text of than it sent; every read waits on its gate. */
  async function reading(): Promise<{ agents: Agents; backend: FakeBackend; reads: number[]; open(): void; text: { now: string } }> {
    const backend = createFakeBackend({ pace: 1, chats: [conversation('big', 'Big', [['user', 'Go'], ['assistant', 'a']])] })
    const reads: number[] = []
    const text = { now: 'abc' }
    let gate = Promise.resolve()
    let release = (): void => {}
    const rpc = {
      ...backend.rpc,
      readItem: async ({ item }: { chat: string; item: number }) => {
        reads.push(item)
        await gate
        return { id: item, chat: 'big', kind: 'assistant' as const, text: text.now, name: '', data: '', done: true }
      }
    }
    const agents = new Agents(rpc, backend.events)
    agents.listen()
    await agents.load()
    await agents.openChat(agents.chats[0]!)
    // the next reads wait until `open` is called
    gate = new Promise<void>((resolve) => (release = resolve))
    return { agents, backend, reads, open: () => release(), text }
  }

  test('is read when the backend says it changed, and shown as it is', async () => {
    const { agents, backend, reads, open } = await reading()
    open()
    backend.emit('item_changed', { chat: 'big', item: 2 })
    await until(() => said(agents).at(-1) === 'assistant: abc')
    expect(reads).toEqual([2])
    expect(agents.failure).toBeNull()
  })

  test('is read one at a time: a change that comes during a read has it read once more, not twice', async () => {
    const { agents, backend, reads, open, text } = await reading()
    backend.emit('item_changed', { chat: 'big', item: 2 })
    backend.emit('item_changed', { chat: 'big', item: 2 })
    backend.emit('item_changed', { chat: 'big', item: 2 })
    expect(reads).toEqual([2])
    text.now = 'abcdef'
    open()
    await until(() => said(agents).at(-1) === 'assistant: abcdef')
    expect(reads).toEqual([2, 2])
  })

  test('is read again when a smaller change of it comes during a read: the read cannot be older than the change', async () => {
    const { agents, backend, reads, open, text } = await reading()
    backend.emit('item_changed', { chat: 'big', item: 2 })
    backend.emit('piece', { chat: 'big', item: 2, text: '!' })
    text.now = 'abc!'
    open()
    await until(() => said(agents).at(-1) === 'assistant: abc!')
    expect(reads).toEqual([2, 2])
  })

  test('is not read when its chat is not the one open', async () => {
    const { agents, backend, reads, open } = await reading()
    open()
    backend.emit('item_changed', { chat: 'elsewhere', item: 2 })
    await Bun.sleep(10)
    expect(reads).toEqual([])
    expect(said(agents)).toEqual(['user: Go', 'assistant: a'])
  })

  test('an item read late goes where its id puts it, not after a later item that arrived whole', async () => {
    const { agents, backend, open } = await reading()
    backend.emit('item_changed', { chat: 'big', item: 3 })
    backend.emit('item', { chat: 'big', item: { id: 4, chat: 'big', kind: 'assistant', text: 'four', name: '', data: '', done: true } })
    expect(agents.open?.items.map((i) => i.id)).toEqual([1, 2, 4])
    open()
    await until(() => agents.open?.items.length === 4)
    expect(agents.open?.items.map((i) => i.id)).toEqual([1, 2, 3, 4])
    expect(said(agents)).toEqual(['user: Go', 'assistant: a', 'assistant: abc', 'assistant: four'])
  })

  /** A window whose reads each wait for the test to answer them, one by one. */
  async function held(): Promise<{ agents: Agents; backend: FakeBackend; reads: { give(text: string, done: boolean): void }[] }> {
    const backend = createFakeBackend({ pace: 1, chats: [conversation('big', 'Big', [['user', 'Go'], ['assistant', 'a']])] })
    const reads: { give(text: string, done: boolean): void }[] = []
    const rpc = {
      ...backend.rpc,
      readItem: ({ item }: { chat: string; item: number }) =>
        new Promise<VosApp.Agents.Item>((resolve) =>
          reads.push({ give: (text, done) => resolve({ id: item, chat: 'big', kind: 'assistant', text, name: '', data: '', done }) })
        )
    }
    const agents = new Agents(rpc, backend.events)
    agents.listen()
    await agents.load()
    await agents.openChat(agents.chats[0]!)
    return { agents, backend, reads }
  }

  test('an answer older than a whole chat installed since is not shown: the item is read again', async () => {
    const { agents, backend, reads } = await held()
    backend.emit('item_changed', { chat: 'big', item: 2 })
    expect(reads).toHaveLength(1)
    // the turn is over: the chat is read as the backend has it, whole
    backend.emit('turn', { chat: 'big' })
    await Bun.sleep(20)
    expect(said(agents).at(-1)).toBe('assistant: a')
    reads[0]!.give('old and unfinished', false)
    await until(() => reads.length === 2)
    expect(said(agents).at(-1)).toBe('assistant: a')
    expect(agents.open?.items.find((i) => i.id === 2)?.done).not.toBe(false)
    reads[1]!.give('the final text', true)
    await until(() => said(agents).at(-1) === 'assistant: the final text')
    expect(agents.open?.items.find((i) => i.id === 2)?.done).toBe(true)
  })

  test('an answer older than a change of the item shown is not shown, and the item is read again', async () => {
    const { agents, backend, reads } = await held()
    backend.emit('item_changed', { chat: 'big', item: 2 })
    backend.emit('item', { chat: 'big', item: { id: 2, chat: 'big', kind: 'assistant', text: 'newer', name: '', data: '', done: true } })
    reads[0]!.give('older', false)
    await until(() => reads.length === 2)
    expect(said(agents).at(-1)).toBe('assistant: newer')
    reads[1]!.give('newest', true)
    await until(() => said(agents).at(-1) === 'assistant: newest')
  })

  test('a failure older than a whole chat installed since is not said', async () => {
    const backend = createFakeBackend({ pace: 1, chats: [conversation('big', 'Big', [['user', 'Go'], ['assistant', 'a']])] })
    const failures: ((e: Error) => void)[] = []
    const rpc = { ...backend.rpc, readItem: () => new Promise<VosApp.Agents.Item>((_, reject) => failures.push(reject)) }
    const agents = new Agents(rpc, backend.events)
    agents.listen()
    await agents.load()
    await agents.openChat(agents.chats[0]!)
    backend.emit('item_changed', { chat: 'big', item: 2 })
    backend.emit('turn', { chat: 'big' })
    await Bun.sleep(20)
    failures[0]!(new Error('too old to matter'))
    await until(() => failures.length === 2)
    expect(agents.failure).toBeNull()
  })

  test('that the backend cannot give is said, when its chat is still open', async () => {
    const backend = createFakeBackend({ pace: 1, chats: [conversation('big', 'Big', [['user', 'Go']])] })
    const agents = new Agents(backend.rpc, backend.events)
    agents.listen()
    await agents.load()
    await agents.openChat(agents.chats[0]!)
    backend.emit('item_changed', { chat: 'big', item: 99 })
    await until(() => agents.failure !== null)
    expect(agents.failure).toBe('There is no such item in that chat.')
  })
})

describe('the host connection', () => {
  test('when it comes back, the open chat is read again', async () => {
    const backend = createFakeBackend({ pace: 1 })
    const agents = await opened(backend)
    // a chat another window started, opened here before its answer came
    const made = await backend.rpc.send({ agent: SCRIPTED.id, text: 'hello' })
    await agents.load()
    await agents.openChat(made.chat)
    await Bun.sleep(20)
    backend.emit('host:reconnected')
    await until(() => said(agents).length === 2)
    expect(said(agents)).toEqual(['user: hello', 'assistant: Hi there'])
  })

  test('after `listen` is stopped, events are not heard', async () => {
    const backend = createFakeBackend({ pace: 1 })
    const agents = new Agents(backend.rpc, backend.events)
    const stop = agents.listen()
    await agents.load()
    agents.startChat(SCRIPTED)
    stop()
    agents.draft = 'hello'
    await agents.send()
    await Bun.sleep(20)
    expect(said(agents)).toEqual(['user: hello'])
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
    const failing = new Agents({ ...backend.rpc, send: () => Promise.reject(new Error('Connection lost')) }, backend.events)
    await failing.load()
    failing.startChat(SCRIPTED)
    failing.draft = 'hello'
    await failing.send()
    expect(failing.draft).toBe('hello')
    expect(failing.failure).toBe('Connection lost')
    expect(failing.open?.items).toEqual([])
    expect(failing.chats).toEqual([]) // and no chat was made
  })

  test('a rejection that is not an Error is shown as text', async () => {
    const backend = createFakeBackend()
    const agents = new Agents({ ...backend.rpc, agents: () => Promise.reject('offline') }, backend.events)
    await agents.load()
    expect(agents.failure).toBe('offline')
  })
})
