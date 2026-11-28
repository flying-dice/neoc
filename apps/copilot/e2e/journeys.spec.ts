/**
 * Copilot key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend, which builds each copilot's image, makes each chat's container and speaks ACP to the agent on
 * that container's standard streams. The Docker it reaches is the journeys' own (`@vos/docker/fake-docker`: the Engine
 * API in this process), which the app is pointed at as a person would, in its address field; and the agent the
 * container runs is the journeys' fake ACP agent (e2e/acp-agent.ts), the program of every container a client attaches
 * to. So the journeys need no Docker, no image and no agent, and what they look at is what the app asked Docker for.
 *
 * The journeys are one person's story, one after another on the shared host: add and build a copilot, give it a skill
 * and chat with it, answer what it asks, delete the chat, and remove the copilot. A Keychain item one of its variables
 * takes its value from is put in the host's keychain (a mock one: e2e/host/start.ts) through the Keychain app's tool.
 */
import { createHash } from 'node:crypto'
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, frameOf, openDesktop, test } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'
import { startFakeDocker, type FakeDocker } from '@vos/docker/fake-docker'
import { FakeAgent } from '../../../e2e/acp-agent'

const ID = 'copilot'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree, its text area the real textarea, its picker the select
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const area = (page: Page, name: string): Locator => part(page, name).locator('textarea')
const picker = (page: Page, name: string): Locator => part(page, name).locator('select')
const thread = (page: Page): Locator => part(page, 'thread')
const input = (page: Page): Locator => field(page, 'composer-input')

const RUN = `${Date.now()}`
const NAME = `Coder ${RUN}`
const DOCKERFILE = `FROM oven/bun:1\nRUN bun add -g fake-acp-agent # ${RUN}\n`
const SKILL = `release-notes-${RUN}`
const SKILL_TEXT = '# Release notes\n\nWrite them from the merged pull requests.'
// the Keychain item a variable takes its value from
const SECRET = { service: `api.example.com-${RUN}`, account: 'ada', password: 'sk-test-7Kq2mVn9' }
// an image of the machine that is no copilot's: removing a copilot leaves it
const OTHER_IMAGE = 'alpine:3.20'

test.describe.configure({ mode: 'serial' })

let docker: FakeDocker
const agent = new FakeAgent([
  [
    (text) => text === 'hello',
    async (turn) => {
      turn.say('Hi ')
      await turn.pause(50)
      turn.say('there')
    }
  ],
  // a permission asked for before a tool call, and what the agent does with the answer
  [
    (text) => text === 'delete old.md',
    async (turn) => {
      turn.update({ sessionUpdate: 'tool_call', toolCallId: 'del1', title: 'Delete old.md', kind: 'delete', status: 'pending' })
      const answer = await turn.ask({ toolCallId: 'del1', title: 'Delete old.md', kind: 'delete' }, [
        { optionId: 'yes', name: 'Allow', kind: 'allow_once' },
        { optionId: 'no', name: 'Reject', kind: 'reject_once' }
      ])
      if (answer.outcome !== 'selected') return 'cancelled'
      const allowed = answer.optionId === 'yes'
      turn.update({ sessionUpdate: 'tool_call_update', toolCallId: 'del1', status: allowed ? 'completed' : 'failed' })
      turn.say(allowed ? 'Deleted.' : 'I left it.')
    }
  ]
])

// what the journeys learn as they go: the copilot's id, its image, the skill's id and the chat's id
let copilot = ''
let image = ''
let skill = ''
let chat = ''
const container = (): string => `vos-copilot-${chat}`

/** The agent is the program of a container: each client that attaches to it is served, while the container is there. */
const serving = new Set<string>()
function serve(name: string): void {
  if (serving.has(name)) return
  serving.add(name)
  void (async () => {
    while (serving.has(name)) agent.serve(await docker.attached(name))
  })()
}

test.beforeAll(async () => {
  docker = await startFakeDocker({ images: [{ tags: [OTHER_IMAGE] }] })
})

test.afterAll(async ({ browser }) => {
  // the app goes back to this machine's own Docker, as it was
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openDesktop(page, { open: [ID] })
  await part(page, 'app').evaluate(async () => {
    const rpc = (globalThis as unknown as { RPC: { send_request(app: string, method: string, params: unknown): Promise<unknown> } }).RPC
    await rpc.send_request('copilot', '_connect', { host: '' })
  })
  await context.close()
  serving.clear()
  await agent.stop()
  await docker.close()
})

/** The app's window, on the journeys' Docker: chosen in the address field while the app has none it can reach. */
async function openCopilot(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'down').or(part(page, 'library'))).toBeVisible()
  if (await part(page, 'down').isVisible()) {
    await field(page, 'host-input').fill(docker.host)
    await part(page, 'host-connect').click()
  }
  await expect(part(page, 'library')).toBeVisible()
}

async function say(page: Page, text: string): Promise<void> {
  await input(page).fill(text)
  await input(page).press('Enter')
}

const turnOver = (page: Page): Promise<void> => expect(part(page, 'composer-send')).toBeVisible()

/** The text of a tar archive's names and files, to look for them in. */
const tarText = (archive: Uint8Array): string => Buffer.from(archive).toString('utf8')

test.describe('Copilot journeys', () => {
  test('CP1: add a copilot with a Dockerfile, a command and its environment, and build its image', async ({ page }) => {
    await callTool('keychain__add_keychain_item', SECRET)
    await openCopilot(page)
    await part(page, 'add').click()
    await field(page, 'form-name').fill(NAME)
    await field(page, 'form-command').fill('fake-acp-agent --stdio')
    await area(page, 'form-dockerfile').fill(DOCKERFILE)
    // a value written out, and one that is a Keychain item's password
    await part(page, 'form-add-env').click()
    await field(page, 'form-env-0-name').fill('GREETING')
    await field(page, 'form-env-0-value').fill('hello')
    await part(page, 'form-add-env').click()
    await field(page, 'form-env-1-name').fill('API_KEY')
    await picker(page, 'form-env-1-source').selectOption({ label: `${SECRET.service} (${SECRET.account})` })
    await expect(part(page, 'form-env-1-value')).toHaveCount(0)

    await part(page, 'form-build').click()
    await expect(part(page, 'build-status')).toHaveText(/Built: vos-copilot-/)
    image = ((await part(page, 'build-status').textContent()) ?? '').replace(/^\s*Built:\s*/, '').trim()
    copilot = /^vos-copilot-(.+):/.exec(image)![1]!
    // tagged by the copilot and the start of its Dockerfile's hash
    expect(image).toBe(`vos-copilot-${copilot}:${createHash('sha256').update(DOCKERFILE).digest('hex').slice(0, 12)}`)
    await expect(part(page, 'build-lines')).toContainText(`Building ${image}`)
    await expect(part(page, 'build-lines')).toContainText(`Built ${image}.`)
    await expect(part(page, `copilot-${copilot}`)).toBeVisible()
    expect(docker.images().flatMap((i) => i.tags)).toContain(image)
  })

  test('CP2: add a skill, give it to the copilot, and chat in a container of its own', async ({ page }) => {
    await openCopilot(page)
    await part(page, 'open-skills').click()
    await part(page, 'add-skill').click()
    await field(page, 'skill-form-name').fill(SKILL)
    await area(page, 'skill-form-text').fill(SKILL_TEXT)
    await part(page, 'skill-form-save').click()
    const row = part(page, 'skills').getByTestId(/^copilot-skill-/).filter({ hasText: SKILL })
    await expect(row).toBeVisible()
    skill = ((await row.getAttribute('data-testid')) ?? '').replace(/^copilot-skill-/, '')

    await part(page, `edit-${copilot}`).click()
    await part(page, `form-skill-${skill}`).click()
    await part(page, 'form-save').click()
    await expect(part(page, 'form')).toHaveCount(0)

    await part(page, `new-chat-${copilot}`).click()
    await say(page, 'hello')
    // the first message makes the chat: its container is named after it, and the agent is the program it runs
    const made = part(page, `copilot-${copilot}`).getByTestId(/^copilot-chat-/)
    await expect(made).toHaveCount(1)
    chat = ((await made.getAttribute('data-testid')) ?? '').replace(/^copilot-chat-/, '')
    serve(container())
    await expect(thread(page)).toContainText('Hi there')
    await turnOver(page)

    // the container: the copilot's image and command, its environment with the Keychain item's password, the chat's
    // volume at the work folder, the labels that say whose it is, and its input kept open for the next attach
    const box = docker.container(container())!
    expect(box).toMatchObject({
      image,
      cmd: ['fake-acp-agent', '--stdio'],
      env: ['GREETING=hello', `API_KEY=${SECRET.password}`],
      labels: { 'vos.app': ID, 'vos.copilot': copilot, 'vos.thread': chat },
      mounts: [{ type: 'volume', source: container(), target: '/workspace', readOnly: false }],
      workingDir: '/workspace',
      openStdin: true,
      running: true
    })
    expect(docker.volumes()).toContain(container())
    // the skill was copied in before the container first started, in a folder of its name under the skills folder
    const uploads = docker.uploads(container())
    expect(uploads).toHaveLength(1)
    expect(uploads[0]).toMatchObject({ path: '/', running: false })
    expect(tarText(uploads[0]!.archive)).toContain(`root/.claude/skills/${SKILL}/SKILL.md`)
    expect(tarText(uploads[0]!.archive)).toContain(SKILL_TEXT)
    // the agent was spoken to as an ACP client does, and its session works in the chat's volume
    expect(agent.received.find((m) => m.method === 'initialize')?.params.clientInfo.name).toBe('vos')
    const session = agent.sessionOf('hello')!
    expect(agent.received.some((m) => m.method === 'session/new' && m.params.cwd === '/workspace')).toBe(true)
    expect(session.cwd).toBe('/workspace')
  })

  test('CP3: the agent asks before it acts, and the answer lets it go on', async ({ page }) => {
    await openCopilot(page)
    await part(page, `chat-${chat}`).click()
    await expect(thread(page)).toContainText('Hi there')
    await say(page, 'delete old.md')
    const question = frame(page).getByTestId(/^copilot-question-\d+$/)
    await expect(question).toContainText('Delete old.md')
    await expect(input(page)).toBeDisabled()
    await question.getByTestId(/-option-0$/).click()
    await expect(thread(page)).toContainText('Deleted.')
    await expect(question).toContainText('Answered: Allow')
    await turnOver(page)
    expect(agent.sessionOf('delete old.md')).toBe(agent.sessionOf('hello'))
  })

  test('CP4: delete the chat, and its container and volume go with it', async ({ page }) => {
    await openCopilot(page)
    await part(page, `chat-${chat}`).click()
    await expect(thread(page)).toContainText('Deleted.')
    const session = agent.sessionOf('hello')!
    await part(page, 'remove-chat').click()
    await part(page, 'confirm-confirm').click()
    await expect(part(page, 'no-chat')).toBeVisible()
    await expect(part(page, `chat-${chat}`)).toHaveCount(0)
    serving.delete(container())
    await expect.poll(() => docker.container(container())).toBeUndefined()
    expect(docker.volumes()).not.toContain(container())
    // the agent's input was closed: its program ended
    await expect.poll(() => session.connection.closed).toBe(true)
  })

  test('CP5: remove the copilot, and its image goes with it', async ({ page }) => {
    await openCopilot(page)
    await part(page, `edit-${copilot}`).click()
    await part(page, 'form-remove').click()
    await part(page, 'confirm-confirm').click()
    await expect(part(page, `copilot-${copilot}`)).toHaveCount(0)
    await expect.poll(() => docker.images().flatMap((i) => i.tags).filter((tag) => tag.startsWith(`vos-copilot-${copilot}:`))).toEqual([])
    expect(docker.images().flatMap((i) => i.tags)).toContain(OTHER_IMAGE)
  })
})
