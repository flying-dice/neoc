import { expect, test } from 'bun:test'
import { Copilots } from './copilot.svelte'
import { blankVariable } from './form'
import { ADMIN, CODER, NO_KEYCHAIN, REVIEW, SECRETS, UNREACHABLE, conversation, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend({ pace: 1 })): Promise<Copilots> {
  const copilots = new Copilots(backend.rpc, backend.events)
  copilots.listen()
  await copilots.load()
  return copilots
}

/** Wait until `done()` holds (the agent and a build go on on timers of their own). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const said = (copilots: Copilots): string[] => copilots.open?.items.map((i) => `${i.kind}: ${i.text}`) ?? []

test('a window reads Docker, the copilots, the skills and the chats', async () => {
  const copilots = await opened(createFakeBackend({ chats: [conversation('a', 'Hi', CODER, [['user', 'Hi']])] }))
  expect(copilots.loaded).toBe(true)
  expect(copilots.engine?.reachable).toBe(true)
  expect(copilots.copilots.map((c) => c.name)).toEqual(['Coder', 'Sysadmin'])
  expect(copilots.skills.map((s) => s.name)).toEqual(['review'])
  expect(copilots.chatsOf('coder').map((c) => c.id)).toEqual(['a'])
  expect(copilots.chatsOf('admin')).toEqual([])
})

test('a Docker that cannot be reached is said, and another can be chosen', async () => {
  const copilots = await opened(createFakeBackend({ engine: UNREACHABLE }))
  expect(copilots.engine?.reachable).toBe(false)
  copilots.hostDraft = 'tcp://nas.local:2375'
  await copilots.connect()
  expect(copilots.engine).toMatchObject({ reachable: false, host: 'tcp://nas.local:2375' })
  copilots.hostDraft = ' tcp://docker:2375 '
  await copilots.connect()
  expect(copilots.engine).toMatchObject({ reachable: true, host: 'tcp://docker:2375' })
  expect(copilots.hostDraft).toBe('tcp://docker:2375')
})

test('a chat is made by its first message, with the copilot it was started with', async () => {
  const copilots = await opened()
  copilots.startChat(ADMIN)
  expect(copilots.open?.chat).toMatchObject({ id: '', copilot_id: 'admin' })
  expect(copilots.chats).toEqual([])
  copilots.draft = 'hello'
  await copilots.send()
  expect(copilots.draft).toBe('')
  await until(() => !copilots.open?.busy && said(copilots).includes('assistant: Hi there'))
  expect(copilots.chatsOf('admin').map((c) => c.title)).toEqual(['hello'])
})

test('a permission is answered, and a turn stopped', async () => {
  const copilots = await opened()
  copilots.startChat(CODER)
  copilots.draft = 'ask'
  await copilots.send()
  await until(() => copilots.open?.waiting === true)
  const question = copilots.open!.items.find((i) => i.kind === 'permission')!
  await copilots.answer(question, 'allow')
  await until(() => said(copilots).includes('assistant: Added.') && !copilots.open!.busy)
  copilots.draft = 'slow'
  await copilots.send()
  await until(() => said(copilots).includes('assistant: Thinking'))
  await copilots.stop()
  await until(() => copilots.open?.busy === false)
})

test('deleting a chat takes it off the list; a new one nothing was said in is only closed', async () => {
  const copilots = await opened(createFakeBackend({ chats: [conversation('a', 'Hi', CODER, [['user', 'Hi']])] }))
  await copilots.openChat(copilots.chats[0]!)
  await copilots.removeChat()
  expect(copilots.open).toBeNull()
  expect(copilots.chats).toEqual([])
  copilots.startChat(CODER)
  await copilots.removeChat()
  expect(copilots.open).toBeNull()
})

test('a copilot is added from the form, and the form closes', async () => {
  const copilots = await opened()
  copilots.editCopilot(null)
  expect(copilots.pane).toBe('copilot')
  expect(copilots.savable).toBe(false)
  Object.assign(copilots.form!, { name: 'Writer', dockerfile: 'FROM alpine\n', command: 'agent --acp' })
  copilots.form!.env.push({ ...blankVariable(), name: 'TOKEN', secret: SECRETS[1]! })
  await copilots.saveCopilot()
  expect(copilots.form).toBeNull()
  expect(copilots.copilots.find((c) => c.name === 'Writer')).toMatchObject({
    command: ['agent', '--acp'],
    env: [{ name: 'TOKEN', secret: SECRETS[1] }]
  })
})

test('a refused form stays open with the reason', async () => {
  const copilots = await opened()
  copilots.editCopilot(CODER)
  copilots.form!.workdir = 'work'
  await copilots.saveCopilot()
  expect(copilots.form).not.toBeNull()
  expect(copilots.failure).toBe('work is not an absolute path.')
})

test("the form reads the keychain's items; without the Keychain app it says why, and takes values written out", async () => {
  const copilots = await opened()
  copilots.editCopilot(CODER)
  await until(() => copilots.secrets.length === 2)
  expect(copilots.secretsProblem).toBeNull()
  const without = await opened(createFakeBackend({ pace: 1, secrets: null }))
  without.editCopilot(null)
  await until(() => without.secretsProblem !== null)
  expect(without.secretsProblem).toBe(NO_KEYCHAIN)
  expect(without.secrets).toEqual([])
})

test("Build saves the form, keeps it open on the copilot, and shows the build's lines until it is done", async () => {
  const copilots = await opened()
  copilots.editCopilot(null)
  Object.assign(copilots.form!, { name: 'Builder', dockerfile: 'FROM alpine\nRUN echo hi\n', command: 'agent' })
  await copilots.build()
  const id = copilots.form!.copilot!.id
  expect(copilots.copilots.find((c) => c.id === id)?.name).toBe('Builder')
  await until(() => copilots.buildOf(id)?.status === 'done')
  expect(copilots.buildOf(id)!.lines).toEqual(['Step 1/2 : FROM alpine', 'Step 2/2 : RUN echo hi', expect.stringMatching(/^Built vos-copilot-/)])
  // the form shows the copilot as it is now: its new image
  await until(() => copilots.editing?.current === true)
})

test('a build that fails says why, and a new one starts its log again', async () => {
  const copilots = await opened()
  copilots.editCopilot(ADMIN)
  copilots.form!.dockerfile = 'FROM debian\nRUN false\n'
  await copilots.build()
  await until(() => copilots.buildOf('admin')?.status === 'failed')
  expect(copilots.buildOf('admin')!.lines.at(-1)).toContain('non-zero code')
  await until(() => copilots.copilots.find((c) => c.id === 'admin')!.problem !== '')
  copilots.form!.dockerfile = 'FROM debian\n'
  await copilots.build()
  await until(() => copilots.buildOf('admin')?.status === 'done')
  expect(copilots.buildOf('admin')!.lines).toHaveLength(2)
})

test('removing a copilot takes its chats with it', async () => {
  const copilots = await opened(createFakeBackend({ chats: [conversation('a', 'Hi', CODER, [['user', 'Hi']]), conversation('b', 'Up?', ADMIN, [['user', 'Up?']])] }))
  copilots.editCopilot(CODER)
  await copilots.removeCopilot()
  expect(copilots.form).toBeNull()
  expect(copilots.copilots.map((c) => c.id)).toEqual(['admin'])
  expect(copilots.chats.map((c) => c.id)).toEqual(['b'])
})

test('skills are added, changed and removed in the library, and a removed one leaves the copilots that used it', async () => {
  const copilots = await opened()
  copilots.openLibrary()
  expect(copilots.pane).toBe('skills')
  copilots.editSkill(null)
  expect(copilots.pane).toBe('skill')
  Object.assign(copilots.skillForm!, { name: 'deploy', text: '# Deploy' })
  await copilots.saveSkill()
  expect(copilots.pane).toBe('skills')
  expect(copilots.skills.map((s) => s.name)).toEqual(['deploy', 'review'])
  copilots.editSkill(REVIEW)
  copilots.skillForm!.name = 'deploy'
  await copilots.saveSkill()
  expect(copilots.failure).toBe('There is a skill named deploy already.')
  copilots.closeSkill()
  expect(copilots.failure).toBeNull()
  copilots.editSkill(REVIEW)
  await copilots.removeSkill()
  expect(copilots.skills.map((s) => s.name)).toEqual(['deploy'])
  expect(copilots.copilots.find((c) => c.id === 'coder')!.skills).toEqual([])
})

test('a backend that cannot be reached is said', async () => {
  const copilots = await opened(createFailingBackend('The host is not answering'))
  expect(copilots.loaded).toBe(false)
  expect(copilots.failure).toBe('The host is not answering')
})

test('a host connection that came back is read again', async () => {
  const backend = createFakeBackend({ pace: 1 })
  const copilots = await opened(backend)
  await backend.rpc.addCopilot({ name: 'Elsewhere', dockerfile: 'FROM alpine', command: ['agent'] })
  backend.emit('host:reconnected')
  await until(() => copilots.copilots.some((c) => c.name === 'Elsewhere'))
})
