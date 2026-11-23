/**
 * Agents key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window, and
 * its Luau backend, which keeps the agents and the chats and runs each turn against the agent's AG-UI endpoint. The
 * agent is aimock's AG-UI agent (`AGUIMock`), started here: what the person says picks the run it answers with.
 *
 * The agents and chats live on the shared host, so each test adds an agent of its own and starts its own chat.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { AGUIMock, type AGUIEvent, type AGUIRunAgentInput } from '@copilotkit/aimock'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'

const ID = 'agents'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
const thread = (page: Page): Locator => part(page, 'thread')
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const input = (page: Page): Locator => field(page, 'composer-input')
const agentRow = (page: Page, name: string): Locator => frame(page).getByTestId(/^agents-agent-/).filter({ hasText: name })

const agent = new AGUIMock()
let endpoint = ''
/** Every run the agent was asked for, as it was sent. */
const asked: AGUIRunAgentInput[] = []

const lastSaid = (input: AGUIRunAgentInput): string => {
  const last = input.messages?.at(-1)
  return last?.role === 'user' && typeof last.content === 'string' ? last.content : ''
}
const run = (...events: AGUIEvent[]): AGUIEvent[] => [
  { type: 'RUN_STARTED', threadId: 't', runId: 'r' },
  ...events,
  ...(events.some((e) => e.type === 'RUN_FINISHED' || e.type === 'RUN_ERROR') ? [] : [{ type: 'RUN_FINISHED', threadId: 't', runId: 'r' } as AGUIEvent])
]
const says = (id: string, text: string): AGUIEvent[] => [
  { type: 'TEXT_MESSAGE_START', messageId: id, role: 'assistant' },
  { type: 'TEXT_MESSAGE_CONTENT', messageId: id, delta: text },
  { type: 'TEXT_MESSAGE_END', messageId: id }
]

const SURFACE = [
  { version: 'v0.9', createSurface: { surfaceId: 'booking', catalogId: 'basic' } },
  {
    version: 'v0.9',
    updateComponents: {
      surfaceId: 'booking',
      components: [
        { id: 'root', component: 'Column', children: ['title', 'who', 'confirm'] },
        { id: 'title', component: 'Text', text: { call: 'formatString', args: { value: 'A table for ${/name}' } } },
        { id: 'who', component: 'TextField', label: 'Name', value: { path: '/name' } },
        { id: 'confirm', component: 'Button', child: 'label', action: { event: { name: 'confirm', context: { name: { path: '/name' } } } } },
        { id: 'label', component: 'Text', text: 'Confirm' }
      ]
    }
  },
  { version: 'v0.9', updateDataModel: { surfaceId: 'booking', value: { name: 'Ada' } } }
]

test.beforeAll(async () => {
  // the first fixture that matches answers: this one matches nothing, and keeps what was asked
  agent.onPredicate((input) => {
    asked.push(input)
    return false
  }, [])
  agent.onMessage('hello', 'Hi there')
  agent.onPredicate((input) => lastSaid(input).startsWith('remember'), run(...says('m1', 'I will remember.')))
  // a long answer, a piece every second: there is time to stop it
  agent.onPredicate(
    (input) => lastSaid(input) === 'slow',
    run({ type: 'TEXT_MESSAGE_START', messageId: 's1', role: 'assistant' }, ...Array.from({ length: 30 }, (_, i): AGUIEvent => ({ type: 'TEXT_MESSAGE_CONTENT', messageId: 's1', delta: `${i + 1} ` }))),
    1000
  )
  // everything an agent can put in a chat besides its words
  agent.onPredicate(
    (input) => lastSaid(input) === 'show your work',
    run(
      { type: 'STEP_STARTED', stepName: 'plan' },
      { type: 'REASONING_START', messageId: 'r1' },
      { type: 'REASONING_MESSAGE_START', messageId: 'r1', role: 'reasoning' },
      { type: 'REASONING_MESSAGE_CONTENT', messageId: 'r1', delta: 'I should look it up.' },
      { type: 'REASONING_MESSAGE_END', messageId: 'r1' },
      { type: 'REASONING_END', messageId: 'r1' },
      { type: 'TOOL_CALL_START', toolCallId: 'c1', toolCallName: 'web_search' },
      { type: 'TOOL_CALL_ARGS', toolCallId: 'c1', delta: '{"q":"weather"}' },
      { type: 'TOOL_CALL_END', toolCallId: 'c1' },
      { type: 'TOOL_CALL_RESULT', messageId: 'tr1', toolCallId: 'c1', content: 'sunny', role: 'tool' },
      { type: 'STATE_SNAPSHOT', snapshot: { looked: ['weather'] } },
      { type: 'STATE_DELTA', delta: [{ op: 'add', path: '/looked/-', value: 'tides' }] },
      { type: 'ACTIVITY_SNAPSHOT', messageId: 'a1', activityType: 'PLAN', content: { steps: ['look it up'] } },
      { type: 'ACTIVITY_DELTA', messageId: 'a1', activityType: 'PLAN', patch: [{ op: 'add', path: '/steps/-', value: 'say it' }] },
      { type: 'CUSTOM', name: 'usage', value: { tokens: 412 } },
      { type: 'STEP_FINISHED', stepName: 'plan' },
      ...says('m2', 'It is sunny.')
    )
  )
  agent.onPredicate((input) => lastSaid(input) === 'fail', run({ type: 'RUN_ERROR', message: 'The model is overloaded.', code: 'overloaded' }))
  // a tool of an app on this machine: asked for in one run, and its result read in the next
  agent.onPredicate(
    (input) => input.messages?.at(-1)?.role === 'tool',
    run(...says('m3', 'The Calculator says 84.'))
  )
  agent.onPredicate(
    (input) => lastSaid(input) === 'what is 12 times 7',
    run(
      { type: 'TOOL_CALL_START', toolCallId: 'calc1', toolCallName: 'calculator__calculate' },
      { type: 'TOOL_CALL_ARGS', toolCallId: 'calc1', delta: '{"expression":"12 * 7"}' },
      { type: 'TOOL_CALL_END', toolCallId: 'calc1' }
    )
  )
  // a run that asks for a tool and then fails: the id is one another run also uses
  agent.onPredicate(
    (input) => lastSaid(input) === 'ask and fail',
    run(
      { type: 'TOOL_CALL_START', toolCallId: 'calc1', toolCallName: 'calculator__calculate' },
      { type: 'TOOL_CALL_ARGS', toolCallId: 'calc1', delta: '{"expression":"6 * 7"}' },
      { type: 'TOOL_CALL_END', toolCallId: 'calc1' },
      { type: 'RUN_ERROR', message: 'The model gave up.' }
    )
  )
  // the whole conversation again, as many agents send it at the end of a run: an earlier answer and a new one
  agent.onPredicate(
    (input) => lastSaid(input) === 'recap',
    run({
      type: 'MESSAGES_SNAPSHOT',
      messages: [
        { id: 'm1', role: 'assistant', content: 'I will remember.' },
        { id: 'm9', role: 'assistant', content: 'That is all so far.' }
      ]
    } as AGUIEvent)
  )
  // the other ways an agent can say the same things: in chunks, as "thinking" (the older reasoning), through
  // subagents, and with what is not for the person (an encrypted value, an event of its framework)
  agent.onPredicate(
    (input) => lastSaid(input) === 'show the rest',
    run(
      { type: 'THINKING_START', title: 'Planning' },
      { type: 'THINKING_TEXT_MESSAGE_START' },
      { type: 'THINKING_TEXT_MESSAGE_CONTENT', delta: 'Two helpers will do.' },
      { type: 'THINKING_TEXT_MESSAGE_END' },
      { type: 'THINKING_END' },
      { type: 'REASONING_MESSAGE_CHUNK', messageId: 'r7', delta: 'One reads, ' },
      { type: 'REASONING_MESSAGE_CHUNK', delta: 'one writes.' },
      { type: 'REASONING_ENCRYPTED_VALUE', subtype: 'message', entityId: 'r7', encryptedValue: 'c2VjcmV0' },
      { type: 'SUBAGENT_STARTED', subagentRunId: 's1', name: 'researcher', description: 'Reads the sources' },
      { type: 'SUBAGENT_STARTED', subagentRunId: 's2', name: 'writer', description: 'Writes it up' },
      { type: 'TOOL_CALL_CHUNK', toolCallId: 'c7', toolCallName: 'library_search', delta: '{"q":' },
      { type: 'TOOL_CALL_CHUNK', delta: '"tides"}' },
      { type: 'TOOL_CALL_RESULT', messageId: 'tr7', toolCallId: 'c7', content: 'two books', role: 'tool' },
      { type: 'SUBAGENT_FINISHED', subagentRunId: 's1', result: { ok: true } },
      { type: 'SUBAGENT_ERROR', subagentRunId: 's2', message: 'The writer lost its place.' },
      { type: 'RAW', event: { vendorOnly: 'not for the person' }, source: 'framework' },
      { type: 'TEXT_MESSAGE_CHUNK', messageId: 'm7', role: 'assistant', delta: 'Found ' },
      { type: 'TEXT_MESSAGE_CHUNK', delta: 'two books.' }
    ) as AGUIEvent[]
  )
  // a question the run stops at, and the run that goes on with its answer
  agent.onPredicate((input) => (input.resume?.length ?? 0) > 0, run(...says('m4', 'Sent.')))
  agent.onPredicate(
    (input) => lastSaid(input) === 'send the report',
    run({
      type: 'RUN_FINISHED',
      threadId: 't',
      runId: 'r',
      outcome: { type: 'interrupt', interrupts: [{ id: 'int1', reason: 'approval', message: 'Send the report to everyone?' }] }
    } as AGUIEvent)
  )
  // a surface, and the run its button starts
  agent.onPredicate(
    (input) => typeof input.forwardedProps === 'object' && input.forwardedProps !== null && 'a2uiAction' in input.forwardedProps,
    run(...says('m5', 'Booked.'))
  )
  agent.onPredicate(
    (input) => lastSaid(input) === 'book a table',
    run({ type: 'ACTIVITY_SNAPSHOT', messageId: 'surface1', activityType: 'a2ui-surface', content: { a2ui_operations: SURFACE } })
  )
  endpoint = await agent.start()
})
test.afterAll(() => agent.stop())

async function openAgents(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
}

/** Add an agent of this test's own, at aimock's endpoint (or the one given), and return its name. */
async function addAgent(page: Page, url = endpoint, token = ''): Promise<string> {
  const name = `Agent ${Date.now()}-${Math.floor(Math.random() * 1000)}`
  await part(page, 'add').click()
  await field(page, 'form-name').fill(name)
  await field(page, 'form-url').fill(url)
  if (token !== '') await field(page, 'form-token').fill(token)
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
async function chatting(page: Page): Promise<string> {
  await openAgents(page)
  const name = await addAgent(page)
  await newChat(page, name)
  return name
}

const turnOver = (page: Page): Promise<void> => expect(part(page, 'composer-send')).toBeVisible()

test.describe('Agents journeys', () => {
  test('AG1: open Agents from the Home Screen, add an agent by its name and endpoint, and see it', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(part(page, 'no-chat')).toBeVisible()
    await part(page, 'add').click()
    // an endpoint that is no web address is refused, with the reason, and the form stays
    await field(page, 'form-name').fill('Nobody')
    await field(page, 'form-url').fill('ftp://example.com')
    await part(page, 'form-save').click()
    await expect(part(page, 'form-agent')).toContainText('The endpoint must be an http:// or https:// URL.')
    await part(page, 'form-cancel').click()
    const name = await addAgent(page, endpoint, 'a-secret-token')
    await expect(agentRow(page, name)).toContainText(endpoint)
  })

  test('AG2: start a chat and talk to an agent; the first message names the chat', async ({ page }) => {
    await chatting(page)
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
    await expect(part(page, 'title')).toHaveText('hello')
    await expect(part(page, 'chats')).toContainText('hello')
    await turnOver(page)
    await say(page, 'remember this')
    await expect(thread(page)).toContainText('I will remember.')
    await expect(part(page, 'title')).toHaveText('hello') // only the first message names it
    // the second run carried the conversation so far, under the chat's own thread
    const second = asked.findLast((input) => lastSaid(input) === 'remember this')
    expect(second?.messages?.map((m) => `${m.role}: ${String(m.content)}`)).toEqual(['user: hello', 'assistant: Hi there', 'user: remember this'])
  })

  test('AG3: stop an agent that is still answering; what it said stays', async ({ page }) => {
    await chatting(page)
    await say(page, 'slow')
    await expect(part(page, 'composer-stop')).toBeVisible()
    await expect(thread(page)).toContainText('1 ')
    await part(page, 'composer-stop').click()
    await expect(part(page, 'composer-stop')).toHaveCount(0)
    await turnOver(page)
    await expect(thread(page)).toContainText('1 ')
    await expect(frame(page).getByTestId(/^agents-item-error-/)).toHaveCount(0)
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

  test('AG6: see what the agent did: its reasoning, the tool it used, its steps, its activity and the state it shares', async ({ page }) => {
    await chatting(page)
    await say(page, 'show your work')
    await expect(thread(page)).toContainText('It is sunny.')
    await turnOver(page)
    const item = (kind: string): Locator => frame(page).getByTestId(new RegExp(`^agents-item-${kind}-\\d+$`))
    await expect(item('step')).toContainText('plan')
    await expect(item('step')).toContainText('done')
    await item('reasoning').locator('summary').click()
    await expect(item('reasoning')).toContainText('I should look it up.')
    await expect(item('tool')).toContainText('web_search')
    await item('tool').locator('summary').click()
    await expect(item('tool')).toContainText('"q": "weather"')
    await expect(item('tool')).toContainText('sunny')
    // the activity as its delta left it, and the state as its delta left it
    await expect(item('activity')).toContainText('say it')
    await expect(item('custom')).toContainText('usage')
    await part(page, 'state-summary').click()
    await expect(part(page, 'state')).toContainText('tides')
  })

  test('AG7: the agent uses a tool of an app on this machine, and answers with its result', async ({ page }) => {
    await chatting(page)
    await say(page, 'what is 12 times 7')
    await expect(thread(page)).toContainText('The Calculator says 84.')
    const tool = frame(page).getByTestId(/^agents-item-tool-\d+$/)
    await expect(tool).toContainText('calculator__calculate')
    await expect(tool).toContainText('an app on this machine')
    await tool.locator('summary').click()
    await expect(tool).toContainText('"result": 84')
    // every run offers the tools of the apps, and the second carried the result back
    const first = asked.findLast((input) => lastSaid(input) === 'what is 12 times 7')
    expect(first?.tools?.map((t) => t.name)).toContain('calculator__calculate')
    const second = asked.findLast((input) => input.messages?.at(-1)?.role === 'tool')
    expect(String(second?.messages?.at(-1)?.content)).toContain('"result":84')
  })

  test('AG8: the agent stops at a question; the person answers it and the agent goes on', async ({ page }) => {
    await chatting(page)
    await say(page, 'send the report')
    const question = frame(page).getByTestId(/^agents-question-\d+$/)
    await expect(question).toContainText('Send the report to everyone?')
    // nothing else is sent until it is answered
    await expect(input(page)).toBeDisabled()
    await question.locator('input').fill('Only to Ada')
    await question.getByTestId(/-answer$/).click()
    await expect(thread(page)).toContainText('Sent.')
    await expect(question).toContainText('Answered')
    expect(asked.findLast((input) => (input.resume?.length ?? 0) > 0)?.resume).toEqual([{ interruptId: 'int1', status: 'resolved', payload: 'Only to Ada' }])
  })

  test('AG9: the agent draws a surface (A2UI); the person fills it in and its button tells the agent', async ({ page }) => {
    await chatting(page)
    await say(page, 'book a table')
    const surface = part(page, 'surface-booking')
    await expect(surface).toContainText('A table for Ada')
    await turnOver(page)
    await part(page, 'a2ui-booking-who').locator('input').fill('Grace')
    await expect(surface).toContainText('A table for Grace')
    await part(page, 'a2ui-booking-confirm').click()
    await expect(thread(page)).toContainText('Booked.')
    const action = asked.findLast((input) => typeof input.forwardedProps === 'object' && input.forwardedProps !== null && 'a2uiAction' in input.forwardedProps)
    expect(action?.forwardedProps).toMatchObject({
      a2uiAction: { userAction: { name: 'confirm', surfaceId: 'booking', sourceComponentId: 'confirm', context: { name: 'Grace' } } }
    })
  })

  test('AG10: a run that fails, and an endpoint that is not there, say so in the chat', async ({ page }) => {
    await chatting(page)
    await say(page, 'fail')
    await expect(frame(page).getByTestId(/^agents-item-error-/)).toContainText('The model is overloaded.')
    await turnOver(page)
    // nothing listens at port 1
    const nobody = await addAgent(page, 'http://127.0.0.1:1/agent')
    await newChat(page, nobody)
    await say(page, 'hello')
    await expect(frame(page).getByTestId(/^agents-item-error-/)).toContainText("The agent's endpoint could not be reached")
    await turnOver(page)
  })

  test('AG12: a tool asked for by a run that failed is not run, then or in a later turn', async ({ page }) => {
    await chatting(page)
    await say(page, 'ask and fail')
    await expect(frame(page).getByTestId(/^agents-item-error-/)).toContainText('The model gave up.')
    await turnOver(page)
    // a later turn asks for a tool under the same id: that one runs, and the failed run's call still has not
    await say(page, 'what is 12 times 7')
    await expect(thread(page)).toContainText('The Calculator says 84.')
    const tools = frame(page).getByTestId(/^agents-item-tool-\d+$/)
    await expect(tools).toHaveCount(2)
    await tools.nth(0).locator('summary').click()
    await tools.nth(1).locator('summary').click()
    await expect(tools.nth(1)).toContainText('"result": 84')
    await expect(tools.nth(0)).not.toContainText('"result"')
    const carried = asked.findLast((input) => input.messages?.at(-1)?.role === 'tool')
    expect(carried?.messages?.filter((m) => m.role === 'tool')).toHaveLength(1)
  })

  test('AG13: an agent that sends the whole conversation again does not say its earlier answers twice', async ({ page }) => {
    await chatting(page)
    await say(page, 'remember the milk')
    await expect(thread(page)).toContainText('I will remember.')
    await turnOver(page)
    await say(page, 'recap')
    await expect(thread(page)).toContainText('That is all so far.')
    await turnOver(page)
    await expect(thread(page).getByText('I will remember.')).toHaveCount(1)
  })

  test('AG14: an agent that answers in chunks, thinks aloud and sends helpers is shown the same way', async ({ page }) => {
    await chatting(page)
    await say(page, 'show the rest')
    // a chunk without an id goes on with the message before it
    await expect(thread(page)).toContainText('Found two books.')
    await turnOver(page)
    const item = (kind: string): Locator => frame(page).getByTestId(new RegExp(`^agents-item-${kind}-\\d+$`))
    await expect(item('reasoning')).toHaveCount(2)
    await item('reasoning').nth(0).locator('summary').click()
    await expect(item('reasoning').nth(0)).toContainText('Two helpers will do.')
    await item('reasoning').nth(1).locator('summary').click()
    await expect(item('reasoning').nth(1)).toContainText('One reads, one writes.')
    await expect(item('tool')).toContainText('library_search')
    await item('tool').locator('summary').click()
    await expect(item('tool')).toContainText('tides')
    await expect(item('tool')).toContainText('two books')
    await expect(item('subagent')).toHaveCount(2)
    await expect(item('subagent').nth(0)).toContainText('researcher')
    await expect(item('subagent').nth(0)).toContainText('done')
    await expect(item('subagent').nth(1)).toContainText('writer')
    await expect(item('subagent').nth(1)).toContainText('The writer lost its place.')
    // what is not for the person is not in the chat
    await expect(thread(page)).not.toContainText('c2VjcmV0')
    await expect(thread(page)).not.toContainText('not for the person')
    await expect(frame(page).getByTestId(/^agents-item-(custom|error)-/)).toHaveCount(0)
  })

  test('AG11: change an agent and remove it; a chat with it can still be read, and deleted', async ({ page }) => {
    const name = await chatting(page)
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
    await agentRow(page, name).getByTestId(/^agents-edit-/).click()
    await expect(field(page, 'form-name')).toHaveValue(name)
    await field(page, 'form-name').fill(`${name} renamed`)
    await part(page, 'form-save').click()
    await expect(agentRow(page, `${name} renamed`)).toBeVisible()
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
})
