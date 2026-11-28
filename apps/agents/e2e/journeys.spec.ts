/**
 * Agents key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window, and
 * its Luau backend, which keeps the agents and the chats and runs each turn against an ACP agent over a WebSocket. The
 * agent is a fake one started here (e2e/acp-agent.ts): what the person says picks what it does.
 *
 * The agents and chats live on the shared host, so each test adds an agent of its own and starts its own chat.
 */
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { startFakeDocker, type FakeDocker } from '@vos/docker/fake-docker'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool, callToolAt } from '../../../e2e/host/pair'
import { FakeAgent, type Message, type Session } from '../../../e2e/acp-agent'

const ID = 'agents'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
const thread = (page: Page): Locator => part(page, 'thread')
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const input = (page: Page): Locator => field(page, 'composer-input')
// the kit's picker holds the real select
const picker = (page: Page, name: string): Locator => part(page, name).locator('select')
const agentRow = (page: Page, name: string): Locator => frame(page).getByTestId(/^agents-agent-/).filter({ hasText: name })
const item = (page: Page, kind: string): Locator => frame(page).getByTestId(new RegExp(`^agents-item-${kind}-\\d+$`))

const agent = new FakeAgent([
  [
    (text) => text === 'hello',
    async (turn) => {
      turn.say('Hi ')
      await turn.pause(50)
      turn.say('there')
    }
  ],
  [(text) => text.startsWith('remember'), async (turn) => turn.say('I will remember.')],
  // a long answer, a piece every half second, until it is stopped
  [
    (text) => text === 'slow',
    async (turn) => {
      for (let i = 1; i <= 60 && !turn.cancelled; i++) {
        turn.say(`${i} `)
        await turn.pause(500)
      }
    }
  ],
  // what an agent does besides talking: it thinks, plans, changes a file, and gives a link
  [
    (text) => text === 'show your work',
    async (turn) => {
      turn.update({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'The list is in list.md.' } })
      turn.update({
        sessionUpdate: 'plan',
        entries: [
          { content: 'Read list.md', priority: 'high', status: 'in_progress' },
          { content: 'Add milk', priority: 'medium', status: 'pending' }
        ]
      })
      turn.update({ sessionUpdate: 'tool_call', toolCallId: 'edit1', title: 'Add milk to list.md', kind: 'edit', status: 'in_progress', locations: [{ path: '/home/ada/list.md', line: 2 }] })
      turn.update({
        sessionUpdate: 'tool_call_update',
        toolCallId: 'edit1',
        status: 'completed',
        content: [{ type: 'diff', path: '/home/ada/list.md', oldText: '# Shopping\nBread\n', newText: '# Shopping\nBread\nMilk\n' }]
      })
      turn.update({
        sessionUpdate: 'plan',
        entries: [
          { content: 'Read list.md', priority: 'high', status: 'completed' },
          { content: 'Add milk', priority: 'medium', status: 'completed' }
        ]
      })
      // what the app does not show
      turn.update({ sessionUpdate: 'available_commands_update', availableCommands: [{ name: 'review', description: 'Review the changes' }] })
      turn.update({ sessionUpdate: 'usage_update', used: 412 })
      turn.say('Milk is on the **list**.')
      turn.update({ sessionUpdate: 'agent_message_chunk', content: { type: 'resource_link', uri: 'https://example.com/list', name: 'The list' } })
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
  ],
  // a tool of an app on this machine, called over the MCP server the session was given, with its bearer token
  [
    (text) => text === 'what is 12 times 7',
    async (turn) => {
      const vos = turn.session.mcpServers.find((server: { name: string }) => server.name === 'vos')
      const authorization = vos.headers.find((header: { name: string }) => header.name === 'Authorization').value
      turn.update({ sessionUpdate: 'tool_call', toolCallId: 'calc1', title: 'calculator__calculate', kind: 'other', status: 'in_progress', rawInput: { expression: '12 * 7' } })
      const result = await callToolAt({ url: vos.url, headers: { Authorization: authorization } }, 'calculator__calculate', { expression: '12 * 7' })
      turn.update({ sessionUpdate: 'tool_call_update', toolCallId: 'calc1', status: 'completed', content: [{ type: 'content', content: { type: 'text', text: result } }] })
      turn.say(`The Calculator says: ${result}`)
    }
  ],
  // several tools one after another, one of which fails, between two things the agent says
  [
    (text) => text === 'tidy up',
    async (turn) => {
      turn.say('Looking.')
      turn.update({ sessionUpdate: 'tool_call', toolCallId: 'r1', title: 'Read a.md', kind: 'read', status: 'completed' })
      turn.update({ sessionUpdate: 'tool_call', toolCallId: 'r2', title: 'Read b.md', kind: 'read', status: 'completed' })
      turn.update({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'b.md is stale.' } })
      turn.update({ sessionUpdate: 'tool_call', toolCallId: 'x1', title: 'rm b.md', kind: 'execute', status: 'in_progress', rawInput: { command: 'rm b.md' } })
      turn.update({ sessionUpdate: 'tool_call_update', toolCallId: 'x1', status: 'failed', rawOutput: { exit: 1 } })
      turn.say('b.md could not be removed.')
    }
  ],
  [
    (text) => text === 'fail',
    async () => {
      throw { code: -32603, message: 'The model is overloaded.' }
    }
  ],
  [
    (text) => text === 'stop short',
    async (turn) => {
      turn.say('Half of it')
      return 'max_tokens'
    }
  ]
])
let endpoint = ''

test.beforeAll(async () => {
  endpoint = await agent.start()
})
test.afterAll(() => agent.stop())

/** What the app sent the agent, by method. */
const sent = (method: string): Message[] => agent.received.filter((m) => m.method === method)

/** The host-management call the shell makes (`window.__vos.host.api`). */
const hostCall = (page: Page, method: string, params: Record<string, unknown>): Promise<unknown> =>
  page.evaluate(
    ([m, p]) =>
      (window as unknown as { __vos: { host: { api: { call(method: string, params?: unknown): Promise<unknown> } } } }).__vos.host.api.call(m as string, p),
    [method, params] as const
  )

async function openAgents(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
}

/** Add an agent of this test's own, at the fake agent's address (or the one given), and return its name. */
async function addAgent(page: Page, url = endpoint, more: { token?: string; folder?: string } = {}): Promise<string> {
  const name = `Agent ${Date.now()}-${Math.floor(Math.random() * 1000)}`
  await part(page, 'add').click()
  await part(page, 'type-remote').click()
  await field(page, 'form-name').fill(name)
  await field(page, 'form-url').fill(url)
  if (more.folder !== undefined) await field(page, 'form-folder').fill(more.folder)
  if (more.token !== undefined) await field(page, 'form-token').fill(more.token)
  await part(page, 'form-save').click()
  await expect(part(page, 'form')).toHaveCount(0)
  await expect(agentRow(page, name)).toBeVisible()
  return name
}

async function newChat(page: Page, name: string): Promise<void> {
  await agentRow(page, name).click()
  await expect(input(page)).toBeVisible()
  await expect(frame(page).getByTestId(/^agents-message-/)).toHaveCount(0)
}

async function say(page: Page, text: string): Promise<void> {
  await input(page).fill(text)
  await input(page).press('Enter')
}

/** A chat with a new agent, open and empty. */
async function chatting(page: Page, url = endpoint): Promise<string> {
  await openAgents(page)
  const name = await addAgent(page, url)
  await newChat(page, name)
  return name
}

const turnOver = (page: Page): Promise<void> => expect(part(page, 'composer-send')).toBeVisible()

/** A PNG, wider than it is tall, drawn by the browser: orange, with a blue band down the middle. */
async function png(page: Page): Promise<Buffer> {
  const bytes = await page.evaluate(async () => {
    const canvas = new OffscreenCanvas(60, 40)
    const context = canvas.getContext('2d')!
    context.fillStyle = '#ff9500'
    context.fillRect(0, 0, 60, 40)
    context.fillStyle = '#007aff'
    context.fillRect(20, 0, 20, 40)
    const blob = await canvas.convertToBlob({ type: 'image/png' })
    return Array.from(new Uint8Array(await blob.arrayBuffer()))
  })
  return Buffer.from(bytes)
}

test.describe('Agents journeys', () => {
  test('AG1: open Agents from the Home Screen, add an agent by its name, address and folder, and see it', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(part(page, 'no-chat')).toBeVisible()
    // Docker cannot be reached here (e2e/host/start.ts), and no agent needs it: nothing says so
    await expect(part(page, 'down')).toHaveCount(0)
    await part(page, 'add').click()
    // adding an agent starts with its type
    await expect(part(page, 'types')).toBeVisible()
    await expect(part(page, 'down')).toHaveCount(0)
    await part(page, 'type-remote').click()
    await expect(part(page, 'down')).toHaveCount(0)
    // an address that is no WebSocket URL is refused, with the reason, and the form stays
    await field(page, 'form-name').fill('Nobody')
    await field(page, 'form-url').fill('https://example.com/agent')
    await part(page, 'form-save').click()
    await expect(part(page, 'form-agent')).toContainText('The address must be a ws:// or wss:// URL')
    // and so is a folder that is not an absolute path
    await field(page, 'form-url').fill('ws://example.com/acp')
    await field(page, 'form-folder').fill('projects')
    await part(page, 'form-save').click()
    await expect(part(page, 'form-agent')).toContainText('The folder must be an absolute path')
    await part(page, 'form-cancel').click()
    const name = await addAgent(page, endpoint, { token: 'a-secret-token', folder: '/home/ada' })
    await expect(agentRow(page, name)).toContainText(endpoint)
  })

  test('AG2: start a chat and talk to an agent: the first message makes the chat and names it, the answer streams in, the connection stays', async ({ page }) => {
    await openAgents(page)
    const name = await addAgent(page, endpoint, { token: 'a-secret-token', folder: '/home/ada' })
    const itsChats = frame(page).getByTestId(/^agents-chat-/).filter({ hasText: name })
    await newChat(page, name)
    // a chat is made by the first thing said in it: an empty one is not listed, and opening another leaves nothing
    await expect(itsChats).toHaveCount(0)
    await newChat(page, name)
    await expect(itsChats).toHaveCount(0)
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
    await expect(part(page, 'title')).toHaveText('hello')
    await expect(itsChats).toHaveCount(1)
    await expect(part(page, 'chats')).toContainText('hello')
    await turnOver(page)
    // connected with the token on the upgrade and no Origin, as the app, and the session works in the agent's folder
    const session = agent.sessionOf('hello')!
    expect(session.connection.headers.authorization).toBe('Bearer a-secret-token')
    expect(session.connection.headers.origin).toBeUndefined()
    expect(session.cwd).toBe('/home/ada')
    expect(agent.received.findLast((m) => m.method === 'initialize')?.params.clientInfo.name).toBe('vos')
    const connections = sent('initialize').length
    await say(page, 'remember this')
    await expect(thread(page)).toContainText('I will remember.')
    await expect(part(page, 'title')).toHaveText('hello') // only the first message names it
    // the second turn went to the same session, on the same connection
    expect(agent.sessionOf('remember this')).toBe(session)
    expect(sent('initialize').length).toBe(connections)
    expect(session.said).toEqual(['hello', 'remember this'])
  })

  test('AG3: stop an agent that is still answering; what it said stays', async ({ page }) => {
    await chatting(page)
    await say(page, 'slow')
    await expect(part(page, 'composer-stop')).toBeVisible()
    await expect(thread(page)).toContainText('1 ')
    // the line over the composer says the agent is at work, until its turn ends
    await expect(part(page, 'status')).toHaveText('Thinking…')
    await part(page, 'composer-stop').click()
    await expect(part(page, 'composer-stop')).toHaveCount(0)
    await turnOver(page)
    await expect(part(page, 'status')).toHaveText('')
    await expect(thread(page)).toContainText('1 ')
    await expect(frame(page).getByTestId(/^agents-item-error-/)).toHaveCount(0)
    expect(sent('session/cancel').map((m) => m.params.sessionId)).toContain(agent.sessionOf('slow')!.id)
    // and the chat takes the next message
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
  })

  test('AG4: close the window and come back: the chat and its transcript are still there', async ({ page }) => {
    const said = `remember ${Date.now()}`
    await chatting(page)
    await say(page, said)
    await expect(thread(page)).toContainText('I will remember.')
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await openFromHome(page, ID)
    await frame(page).getByTestId(/^agents-chat-/).filter({ hasText: said }).click()
    await expect(thread(page)).toContainText('I will remember.')
  })

  test('AG5: the Agents window at its smallest still works', async ({ page }) => {
    await openAgents(page)
    const name = await addAgent(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(480)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // below 700 wide the list and the chat are one column each: starting a chat opens it, Back returns to the list
    await newChat(page, name)
    await expect(agentRow(page, name)).toBeHidden()
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
    await expect(input(page)).toBeInViewport()
    await part(page, 'back').click()
    await expect(agentRow(page, name)).toBeVisible()
    await expect(part(page, 'chats')).toContainText('hello')
  })

  test('AG6: see what the agent did: what it thought, its plan, the file it changed and a link it gave, its answer as Markdown', async ({ page }) => {
    await chatting(page)
    await say(page, 'show your work')
    await expect(thread(page)).toContainText('Milk is on the list.')
    await expect(frame(page).getByTestId(/^agents-message-assistant-/).locator('strong')).toHaveText('list')
    await turnOver(page)
    await item(page, 'reasoning').locator('summary').click()
    await expect(item(page, 'reasoning')).toContainText('The list is in list.md.')
    // one plan for the turn, as its last update left it
    await expect(item(page, 'plan')).toHaveCount(1)
    await expect(item(page, 'plan')).toContainText('Done Read list.md')
    await expect(item(page, 'plan')).toContainText('Done Add milk')
    // a quiet line: the icon of its kind and its title
    const tool = item(page, 'tool')
    await expect(tool).toContainText('Add milk to list.md')
    await expect(tool.locator('summary ui-icon').first()).toHaveAttribute('name', 'square.and.pencil')
    await expect(tool).not.toContainText(/\((failed|running)\)/)
    await tool.locator('summary').click()
    await expect(tool).toContainText('/home/ada/list.md:2')
    await expect(tool.getByTestId(/-diff-0$/)).toContainText('+Milk')
    await expect(frame(page).getByTestId(/^agents-message-\d+-link$/)).toHaveAttribute('href', 'https://example.com/list')
    // what is not for the person is not in the chat
    await expect(thread(page)).not.toContainText('review')
    await expect(frame(page).getByTestId(/^agents-item-(note|error)-/)).toHaveCount(0)
  })

  test('AG7: the agent uses a tool of an app on this machine over the MCP server it was given', async ({ page }) => {
    await chatting(page)
    await say(page, 'what is 12 times 7')
    await expect(thread(page)).toContainText('The Calculator says')
    await expect(thread(page)).toContainText('84')
    await turnOver(page)
    // the session was given this machine's tools: the host's /mcp, at the address it has on the way to the agent
    const server = agent.sessionOf('what is 12 times 7')!.mcpServers.find((s: { name: string }) => s.name === 'vos')
    expect(server).toMatchObject({ type: 'http', url: expect.stringMatching(/^http:\/\/127\.0\.0\.1:\d+\/mcp$/) })
    const tool = item(page, 'tool')
    await expect(tool).toContainText('calculator__calculate')
    await tool.locator('summary').click()
    await expect(tool).toContainText('"expression": "12 * 7"')
    await expect(tool).toContainText('84')
  })

  test('AG8: the agent asks before it acts; the person allows it once, and refuses it the next time', async ({ page }) => {
    await chatting(page)
    await say(page, 'delete old.md')
    const question = frame(page).getByTestId(/^agents-question-\d+$/)
    await expect(question).toContainText('Delete old.md')
    await expect(question.locator('ui-icon')).toHaveAttribute('name', 'trash')
    // the turn waits for the answer: nothing else is sent until then, and it can be stopped; the question says what
    // the turn waits for, not the line over the composer
    await expect(input(page)).toBeDisabled()
    await expect(part(page, 'composer-stop')).toBeVisible()
    await expect(part(page, 'status')).toHaveText('')
    await question.getByTestId(/-option-0$/).click()
    await expect(thread(page)).toContainText('Deleted.')
    await expect(question).toContainText('Answered: Allow')
    await expect(item(page, 'tool')).not.toContainText(/\((failed|running)\)/)
    await turnOver(page)
    await say(page, 'delete old.md')
    const second = frame(page).getByTestId(/^agents-question-\d+$/).nth(1)
    await second.getByTestId(/-option-1$/).click()
    await expect(thread(page)).toContainText('I left it.')
    await expect(second).toContainText('Answered: Reject')
    await expect(item(page, 'tool').nth(1)).toContainText('· failed')
    // a stop while it waits answers it cancelled, and the turn ends
    await turnOver(page)
    await say(page, 'delete old.md')
    await expect(frame(page).getByTestId(/^agents-question-\d+$/)).toHaveCount(3)
    await part(page, 'composer-stop').click()
    await turnOver(page)
    await expect(frame(page).getByTestId(/^agents-question-\d+$/).nth(2)).toContainText('Not answered')
  })

  test('AG9: after the backend restarts, a chat goes on in its own session; an agent that cannot load one starts afresh and says so', async ({ page }) => {
    await chatting(page)
    await say(page, 'remember the milk')
    await expect(thread(page)).toContainText('I will remember.')
    await turnOver(page)
    const session = agent.sessionOf('remember the milk')!
    await hostCall(page, 'app.reload', { app: ID })
    // the restart closed the connection
    await expect.poll(() => session.connection.closed).toBe(true)
    await say(page, 'remember the bread')
    await expect(thread(page).getByText('I will remember.')).toHaveCount(2)
    expect(sent('session/load').map((m) => m.params.sessionId)).toContain(session.id)
    expect(agent.sessionOf('remember the bread')).toBe(session)
    await expect(frame(page).getByTestId(/^agents-item-note-/)).toHaveCount(0)

    // an agent without session/load
    const name = await addAgent(page, `${endpoint}?load=no`)
    await newChat(page, name)
    await say(page, 'remember the eggs')
    await expect(thread(page)).toContainText('I will remember.')
    await turnOver(page)
    const before = agent.sessionOf('remember the eggs')!
    await hostCall(page, 'app.reload', { app: ID })
    await expect.poll(() => before.connection.closed).toBe(true)
    await say(page, 'remember the cheese')
    await expect(item(page, 'note')).toContainText('it does not remember the earlier messages')
    await expect(thread(page).getByText('I will remember.')).toHaveCount(2)
    expect(agent.sessionOf('remember the cheese')).not.toBe(before)
  })

  test('AG10: an agent that cannot be reached, refuses, fails or stops short says so in the chat', async ({ page }) => {
    await chatting(page)
    await say(page, 'fail')
    await expect(item(page, 'error')).toContainText('The model is overloaded.')
    await turnOver(page)
    await say(page, 'stop short')
    await expect(item(page, 'note')).toContainText('reached the most it may write')
    await turnOver(page)
    // nothing listens at port 1
    const nobody = await addAgent(page, 'ws://127.0.0.1:1/acp')
    await newChat(page, nobody)
    await say(page, 'hello')
    await expect(item(page, 'error')).toContainText('The agent could not be reached')
    await turnOver(page)
    // the agent's server is there, but has no agent at that path
    const elsewhere = await addAgent(page, endpoint.replace(/\/acp$/, '/nobody'))
    await newChat(page, elsewhere)
    await say(page, 'hello')
    await expect(item(page, 'error')).toContainText('refused the connection')
    await expect(item(page, 'error')).toContainText('404')
    await turnOver(page)
  })

  test('AG11: change an agent and remove it; its connection closes, and a chat with it can still be read, and deleted', async ({ page }) => {
    const name = await chatting(page)
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
    await turnOver(page)
    const session = agent.sessionOf('hello')!
    await agentRow(page, name).getByTestId(/^agents-edit-/).click()
    await expect(field(page, 'form-name')).toHaveValue(name)
    await field(page, 'form-name').fill(`${name} renamed`)
    await part(page, 'form-save').click()
    await expect(agentRow(page, `${name} renamed`)).toBeVisible()
    // changed: the next turn connects to it as it is now
    await expect.poll(() => session.connection.closed).toBe(true)
    await agentRow(page, `${name} renamed`).getByTestId(/^agents-edit-/).click()
    await part(page, 'form-remove').click()
    await part(page, 'confirm-confirm').click()
    await expect(agentRow(page, name)).toHaveCount(0)
    // the chat is still there, under the agent's last name, and says why it cannot go on
    await frame(page).getByTestId(/^agents-chat-/).filter({ hasText: `${name} renamed` }).first().click()
    await expect(thread(page)).toContainText('Hi there')
    await say(page, 'hello')
    await expect(part(page, 'failure')).toContainText('That agent was removed.')
    await part(page, 'remove-chat').click()
    await part(page, 'confirm-confirm').click()
    await expect(part(page, 'no-chat')).toBeVisible()
    await expect(frame(page).getByTestId(/^agents-chat-/).filter({ hasText: `${name} renamed` })).toHaveCount(0)
  })

  test('AG12: give an agent a picture, see it in the list, the chats and the chat; remove it, and see its initials', async ({ page }) => {
    await openAgents(page)
    const name = `Agent ${Date.now()}-${Math.floor(Math.random() * 1000)}`
    const initials = `A${name.split(' ')[1]![0]}`
    await part(page, 'add').click()
    await part(page, 'type-remote').click()
    await field(page, 'form-name').fill(name)
    await field(page, 'form-url').fill(endpoint)
    await part(page, 'form-picture-file').setInputFiles({ name: 'face.png', mimeType: 'image/png', buffer: await png(page) })
    // drawn in the page: the square from its middle, 256 pixels across, as WebP
    const preview = part(page, 'form-picture').getByRole('img', { name })
    await expect(preview).toHaveAttribute('src', /^data:image\/webp;base64,/)
    expect(await preview.evaluate((img: HTMLImageElement) => [img.naturalWidth, img.naturalHeight])).toEqual([256, 256])
    await part(page, 'form-save').click()
    await expect(part(page, 'form')).toHaveCount(0)
    const picture = await agentRow(page, name).getByRole('img', { name }).getAttribute('src')
    expect(picture).toMatch(/^data:image\/webp;base64,/)

    // in a chat: beside its name over the chat, and in the list of chats
    await newChat(page, name)
    await expect(part(page, 'avatar-thread').getByRole('img', { name })).toHaveAttribute('src', picture!)
    // the first message makes the chat, and lists it
    await say(page, 'hello')
    await turnOver(page)
    // a chat's row is an option, whose content is its name: the picture is found by what it is
    const chatAvatar = frame(page).getByTestId(/^agents-chat-/).filter({ hasText: name }).getByTestId(/^agents-avatar-chat-/)
    await expect(chatAvatar.locator('img')).toHaveAttribute('src', picture!)

    // change the agent: remove its picture, and its initials are shown everywhere it was
    await agentRow(page, name).getByTestId(/^agents-edit-/).click()
    await part(page, 'form-remove-picture').click()
    await expect(part(page, 'form-picture').getByRole('img', { name })).toHaveAttribute('data-initials', initials)
    await part(page, 'form-save').click()
    await expect(part(page, 'form')).toHaveCount(0)
    await expect(agentRow(page, name).getByRole('img', { name })).toHaveAttribute('data-initials', initials)
    await expect(agentRow(page, name).locator('img')).toHaveCount(0)
    await expect(chatAvatar.locator('[data-initials]')).toHaveAttribute('data-initials', initials)
    await expect(chatAvatar.locator('img')).toHaveCount(0)
    await expect(part(page, 'avatar-thread').getByRole('img', { name })).toHaveAttribute('data-initials', initials)

    // the backend keeps only a PNG, JPEG or WebP picture: an SVG one is refused, in words
    const refused = await part(page, 'app').evaluate(async (_, url) => {
      try {
        const rpc = (globalThis as unknown as { RPC: { send_request(app: string, method: string, params: unknown): Promise<unknown> } }).RPC
        await rpc.send_request('agents', '_add_agent', { name: 'Drawn', url, avatar: 'data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=' })
        return 'added'
      } catch (e) {
        return e instanceof Error ? e.message : String(e)
      }
    }, endpoint)
    expect(refused).toContain('The picture must be a PNG, JPEG or WebP image.')
  })

  test('AG13: tools the agent runs one after another are one quiet row, which opens to each tool, and each tool to what it did', async ({ page }) => {
    await chatting(page)
    await say(page, 'tidy up')
    await expect(thread(page)).toContainText('b.md could not be removed.')
    await turnOver(page)
    const row = frame(page).getByTestId(/^agents-steps-\d+$/)
    await expect(row).toHaveCount(1)
    const summary = row.locator('summary').first()
    await expect(summary).toHaveText(/Read 2 files · Thought once · Ran 1 command\s*· 1 failed/)
    await expect(summary.locator('ui-icon').first()).toHaveAttribute('name', 'doc')
    await expect(item(page, 'tool').first()).toBeHidden()
    await summary.click()
    await expect(item(page, 'tool')).toHaveCount(3)
    await expect(item(page, 'tool').nth(2)).toContainText('rm b.md')
    await expect(item(page, 'tool').nth(2)).toContainText('· failed')
    await item(page, 'tool').nth(2).locator('summary').click()
    await expect(item(page, 'tool').nth(2)).toContainText('"command": "rm b.md"')
    await expect(item(page, 'tool').nth(2)).toContainText('"exit": 1')
  })
})

/** A chat's row in the list, by its agent's name. */
const chatsOf = (page: Page, name: string): Locator => frame(page).getByTestId(/^agents-chat-/).filter({ hasText: name })
/** The id in a row's test id, after its prefix. */
const idOf = async (row: Locator, prefix: string): Promise<string> => ((await row.getAttribute('data-testid')) ?? '').slice(prefix.length)

/**
 * Container agents: each chat is a container of its own, made from the agent's image, and the agent is the container's
 * program, spoken to on its standard streams. The Docker the app reaches is the journeys' own (`@vos/docker/fake-docker`:
 * the Engine API in this process), which the app is pointed at as a person would, in its address field; it has the image
 * the agent uses, as though the Containers app had built it. The program of every container a client attaches to is the
 * fake agent (`serve`). The agent's skill is added in the Skills app as a person adds one, and the Keychain item one of
 * its variables takes its value from is put in the host's keychain (a mock one: e2e/host/start.ts) through the Keychain
 * app's tool.
 *
 * One person's story, one journey after another: add the agent, chat in its container, answer what it asks, delete the
 * chat, and remove the agent.
 */
test.describe('Agents container journeys', () => {
  test.describe.configure({ mode: 'serial' })

  const RUN = `${Date.now()}`
  const NAME = `Coder ${RUN}`
  const IMAGE = 'coder:latest'
  const SKILL = `release-notes-${RUN}`
  const SKILL_TEXT = `---\nname: ${SKILL}\ndescription: Write release notes from the merged pull requests.\n---\n\n# Release notes\n\nRun scripts/log.sh first.\n`
  const SKILL_FILE = { path: 'scripts/log.sh', text: '#!/bin/sh\ngit log --merges --oneline\n' }
  // the Keychain item a variable takes its value from
  const SECRET = { service: `api.example.com-${RUN}`, account: 'ada', password: 'sk-test-7Kq2mVn9' }

  let docker: FakeDocker
  // what the journeys learn as they go: the agent's id, its chat's, and the session the chat's agent has
  let id = ''
  let chat = ''
  let session: Session
  const box = (of = chat): string => `vos-agent-${of}`

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
    docker = await startFakeDocker({ images: [{ tags: [IMAGE] }] })
  })

  test.afterAll(async ({ browser }) => {
    // the app goes back to this machine's own Docker, as it was
    const context = await browser.newContext({
      baseURL: test.info().project.use.baseURL,
      storageState: test.info().project.use.storageState as string
    })
    const page = await context.newPage()
    await openAgents(page)
    await part(page, 'app').evaluate(async () => {
      const rpc = (globalThis as unknown as { RPC: { send_request(app: string, method: string, params: unknown): Promise<unknown> } }).RPC
      await rpc.send_request('agents', '_connect', { host: '' })
    })
    await context.close()
    serving.clear()
    await docker.close()
  })

  /** Start a chat with the agent named, in a container of its own: the first message makes the chat, and its container. */
  async function chatInBox(page: Page, name: string): Promise<string> {
    await newChat(page, name)
    await say(page, 'hello')
    const row = chatsOf(page, name)
    await expect(row).toHaveCount(1)
    const made = await idOf(row, 'agents-chat-')
    serve(box(made))
    await expect(thread(page)).toContainText('Hi there')
    await turnOver(page)
    return made
  }

  /** The text of a tar archive's names and files, to look for them in. */
  const tarText = (archive: Uint8Array): string => Buffer.from(archive).toString('utf8')

  test('AG14: add a container agent: its type, the Docker it uses, an image of it, its command, environment and a skill of the Skills app', async ({ page }) => {
    await callTool('keychain__add_keychain_item', SECRET)
    // the skill, in the Skills app
    await openDesktop(page, { open: ['skills'] })
    const skills = (name: string): Locator => frameOf(page, 'skills').getByTestId(`skills-${name}`)
    await skills('add').click()
    await skills('form-name').locator('input').fill(SKILL)
    await skills('form-text').locator('textarea').fill(SKILL_TEXT)
    await skills('form-add-file').click()
    await skills('form-file-0-path').locator('input').fill(SKILL_FILE.path)
    await skills('form-file-0-text').locator('textarea').fill(SKILL_FILE.text)
    await skills('form-save').click()
    await expect(frameOf(page, 'skills').getByTestId(/^skills-skill-/).filter({ hasText: SKILL })).toBeVisible()

    await openAgents(page)
    await part(page, 'add').click()
    await expect(part(page, 'types').getByTestId(/^agents-type-/)).toHaveCount(3)
    await expect(part(page, 'down')).toHaveCount(0)
    await part(page, 'type-container').click()
    // the app's Docker cannot be reached (e2e/host/start.ts), and a container agent needs it: the journeys' is chosen
    await expect(part(page, 'down')).toBeVisible()
    await field(page, 'host-input').fill(docker.host)
    await part(page, 'host-connect').click()
    await expect(part(page, 'form-container')).toBeVisible()
    await expect(part(page, 'down')).toHaveCount(0)

    await field(page, 'form-name').fill(NAME)
    await picker(page, 'form-image-choice').selectOption(IMAGE)
    await expect(field(page, 'form-image')).toHaveValue(IMAGE)
    await field(page, 'form-command').fill('fake-acp-agent --stdio')
    await expect(field(page, 'form-folder')).toHaveValue('/workspace')
    // a value written out, and one that is a Keychain item's password
    await part(page, 'form-add-env').click()
    await field(page, 'form-env-0-name').fill('GREETING')
    await field(page, 'form-env-0-value').fill('hello')
    await part(page, 'form-add-env').click()
    await field(page, 'form-env-1-name').fill('API_KEY')
    await picker(page, 'form-env-1-source').selectOption({ label: `${SECRET.service} (${SECRET.account})` })
    await expect(part(page, 'form-env-1-value')).toHaveCount(0)
    await part(page, `form-skill-${SKILL}`).click()
    await part(page, 'form-save').click()
    await expect(part(page, 'form')).toHaveCount(0)

    // listed as what it is
    const row = agentRow(page, NAME)
    await expect(row).toBeVisible()
    await expect(row).toHaveJSProperty('subheading', 'Container')
    id = await idOf(row, 'agents-agent-')
    // and what it was given is kept
    await row.getByTestId(/^agents-edit-/).click()
    await expect(field(page, 'form-image')).toHaveValue(IMAGE)
    await expect(field(page, 'form-command')).toHaveValue('fake-acp-agent --stdio')
    await expect(part(page, `form-skill-${SKILL}`)).toHaveJSProperty('checked', true)
    await part(page, 'form-cancel').click()
  })

  test('AG15: chat in a container of its own: the agent is its program, with its environment, a volume at its folder, and its skill', async ({ page }) => {
    await openAgents(page)
    chat = await chatInBox(page, NAME)

    // the container: the agent's image and command, its environment with the Keychain item's password, the chat's
    // volume at the work folder, the labels that say whose it is, and its input kept open for the next attach
    expect(docker.container(box())).toMatchObject({
      image: IMAGE,
      cmd: ['fake-acp-agent', '--stdio'],
      env: ['GREETING=hello', `API_KEY=${SECRET.password}`],
      labels: { 'vos.app': ID, 'vos.agent': id, 'vos.thread': chat },
      mounts: [{ type: 'volume', source: box(), target: '/workspace', readOnly: false }],
      workingDir: '/workspace',
      openStdin: true,
      running: true
    })
    expect(docker.volume(box())?.labels).toEqual({ 'vos.app': ID, 'vos.agent': id, 'vos.thread': chat })
    // the skill was fetched from the Skills app and copied in before the container first started, in a folder of its
    // name under the skills folder, with its file
    const uploads = docker.uploads(box())
    expect(uploads).toHaveLength(1)
    expect(uploads[0]).toMatchObject({ path: '/', running: false })
    const archive = tarText(uploads[0]!.archive)
    expect(archive).toContain(`root/.claude/skills/${SKILL}/SKILL.md`)
    expect(archive).toContain(`root/.claude/skills/${SKILL}/${SKILL_FILE.path}`)
    expect(archive).toContain('Run scripts/log.sh first.')
    expect(archive).toContain('git log --merges --oneline')
    // spoken to as an ACP client does, on the container's streams; its session works in the chat's volume, and is
    // given this machine's tools at the address the container reaches it at
    session = agent.sessionOf('hello')!
    expect(session.connection.headers).toEqual({})
    expect(session.cwd).toBe('/workspace')
    const vos = session.mcpServers.find((s: { name: string }) => s.name === 'vos')
    expect(vos).toMatchObject({ type: 'http', url: expect.stringMatching(/^http:\/\/host\.docker\.internal:\d+\/mcp$/) })
  })

  test('AG16: the container agent asks before it acts, and the answer lets it go on in the same session', async ({ page }) => {
    await openAgents(page)
    await part(page, `chat-${chat}`).click()
    await expect(thread(page)).toContainText('Hi there')
    await say(page, 'delete old.md')
    const question = frame(page).getByTestId(/^agents-question-\d+$/)
    await expect(question).toContainText('Delete old.md')
    await expect(input(page)).toBeDisabled()
    await question.getByTestId(/-option-0$/).click()
    await expect(thread(page)).toContainText('Deleted.')
    await expect(question).toContainText('Answered: Allow')
    await turnOver(page)
    expect(agent.sessionOf('delete old.md')).toBe(session)
  })

  test('AG17: delete the chat, and its container and volume go with it; the agent in it ends', async ({ page }) => {
    await openAgents(page)
    await part(page, `chat-${chat}`).click()
    await expect(thread(page)).toContainText('Deleted.')
    await part(page, 'remove-chat').click()
    await expect(part(page, 'confirm')).toHaveJSProperty('message', '“hello”, everything in it, and its container and data are deleted.')
    await part(page, 'confirm-confirm').click()
    await expect(part(page, 'no-chat')).toBeVisible()
    await expect(part(page, `chat-${chat}`)).toHaveCount(0)
    serving.delete(box())
    await expect.poll(() => docker.container(box())).toBeUndefined()
    expect(docker.volumes()).not.toContain(box())
    // the agent's input was closed: its program ended
    await expect.poll(() => session.connection.closed).toBe(true)
  })

  test("AG18: remove the agent: its chats' containers and volumes go, its image stays; an image Docker does not have is asked for", async ({ page }) => {
    await openAgents(page)
    const other = await chatInBox(page, NAME)
    expect(docker.container(box(other))).toMatchObject({ labels: { 'vos.agent': id }, running: true })
    await agentRow(page, NAME).getByTestId(/^agents-edit-/).click()
    await part(page, 'form-remove').click()
    await expect(part(page, 'confirm')).toHaveJSProperty('message', 'Its chats, with their containers and volumes, are removed with it. Its image stays, in Containers.')
    await part(page, 'confirm-confirm').click()
    await expect(agentRow(page, NAME)).toHaveCount(0)
    await expect(chatsOf(page, NAME)).toHaveCount(0)
    serving.delete(box(other))
    await expect.poll(() => docker.container(box(other))).toBeUndefined()
    expect(docker.volumes()).not.toContain(box(other))
    expect(docker.images().flatMap((i) => i.tags)).toContain(IMAGE)

    // an agent of an image Docker does not have: its chat says where to get it, and nothing is made
    const missing = `missing-${RUN}:1`
    const name = `Missing ${RUN}`
    await part(page, 'add').click()
    await part(page, 'type-container').click()
    await field(page, 'form-name').fill(name)
    await field(page, 'form-image').fill(missing)
    await field(page, 'form-command').fill('agent')
    await part(page, 'form-save').click()
    await expect(agentRow(page, name)).toBeVisible()
    await newChat(page, name)
    await say(page, 'hello')
    await expect(item(page, 'error')).toContainText(`Build or pull ${missing} in Containers first`)
    await turnOver(page)
    const refused = await idOf(chatsOf(page, name), 'agents-chat-')
    expect(docker.container(box(refused))).toBeUndefined()
    expect(docker.volumes()).not.toContain(box(refused))
  })
})

/**
 * A program agent: a command run on this machine, in a folder of it, its standard streams the connection. The program
 * is e2e/acp-relay.ts, which relays them to the fake agent's WebSocket, so the same fake agent serves its chats, and
 * says what the program was started with (an environment variable it is given, on the WebSocket's upgrade).
 */
test.describe('Agents program journeys', () => {
  const RELAY = fileURLToPath(new URL('../../../e2e/acp-relay.ts', import.meta.url)).replaceAll('\\', '/')

  test('AG19: add a program agent: it runs in its folder with its environment, its chat goes on in one session, and it ends with the chat', async ({ page }) => {
    const folder = mkdtempSync(join(tmpdir(), 'vos-agents-program-'))
    try {
      const name = `Relay ${Date.now()}`
      const greeting = `hello-${Date.now()}`
      await openAgents(page)
      await part(page, 'add').click()
      await part(page, 'type-program').click()
      // a program needs no Docker
      await expect(part(page, 'form')).toBeVisible()
      await expect(part(page, 'down')).toHaveCount(0)
      await field(page, 'form-name').fill(name)
      await field(page, 'form-command').fill(`bun ${RELAY} ${endpoint} RELAY_GREETING`)
      // its folder must be one of this machine
      await field(page, 'form-folder').fill(join(folder, 'nowhere'))
      await part(page, 'form-save').click()
      await expect(part(page, 'form-agent')).toContainText('There is no folder')
      await field(page, 'form-folder').fill(folder)
      await part(page, 'form-add-env').click()
      await field(page, 'form-env-0-name').fill('RELAY_GREETING')
      await field(page, 'form-env-0-value').fill(greeting)
      await part(page, 'form-save').click()
      await expect(part(page, 'form')).toHaveCount(0)
      // listed by its command
      await expect(agentRow(page, name)).toHaveJSProperty('subheading', `bun ${RELAY} ${endpoint} RELAY_GREETING`)

      await newChat(page, name)
      await expect(part(page, 'down')).toHaveCount(0)
      const first = `remember the relay ${Date.now()}`
      await say(page, first)
      await expect(thread(page)).toContainText('I will remember.')
      await turnOver(page)
      const session = agent.sessionOf(first)!
      // the program was started with its environment, and its session works in its folder
      expect(session.connection.headers['x-env-relay-greeting']).toBe(greeting)
      expect(session.cwd).toBe(folder)
      const connections = sent('initialize').length
      await say(page, 'remember the second')
      await expect(thread(page).getByText('I will remember.')).toHaveCount(2)
      await turnOver(page)
      // the same program and session
      expect(agent.sessionOf('remember the second')).toBe(session)
      expect(sent('initialize').length).toBe(connections)

      // deleting the chat ends the program: its input closes, and it closes its connection
      await part(page, 'remove-chat').click()
      await part(page, 'confirm-confirm').click()
      await expect(part(page, 'no-chat')).toBeVisible()
      await expect.poll(() => session.connection.closed).toBe(true)

      // a program that is not there says so in its chat
      const nobody = `Nobody ${Date.now()}`
      await part(page, 'add').click()
      await part(page, 'type-program').click()
      await field(page, 'form-name').fill(nobody)
      await field(page, 'form-command').fill('no-such-agent-program --stdio')
      await field(page, 'form-folder').fill(folder)
      await part(page, 'form-save').click()
      await expect(agentRow(page, nobody)).toBeVisible()
      await newChat(page, nobody)
      await say(page, 'hello')
      await expect(item(page, 'error')).toContainText('There is no program no-such-agent-program on this machine')
      await turnOver(page)
    } finally {
      rmSync(folder, { recursive: true, force: true })
    }
  })
})
