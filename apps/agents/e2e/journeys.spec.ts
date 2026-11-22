/**
 * Agents key user journeys (../JOURNEYS.md; a test's id is its journey's id), end to end: the real shell, the real host,
 * the Agents app's own page in a frame of its window, and an ACP agent on the host's roster that is a script, not a model
 * (`crates/vos-acp/src/bin/fake_agent.rs`; the prompt text picks what it does). The page reaches the conversation through
 * the shell's `vos/agui/*` services (the shell's own requests to `/agui`); the permission prompt is an AG-UI interrupt.
 *
 * The chats live on the shared host, so each test starts its own chat and finds it by what it said. The Agents window
 * reopens on the chat it last had open, which the app's backend remembers, so these tests run one after another.
 */
import type { FrameLocator, Page } from '@playwright/test'
import {
  chooseLayout,
  expect,
  expectNoOverflow,
  frameOf,
  openDesktop,
  resizeWindow,
  test,
  windowOf
} from '../../../e2e/fixtures'

const ID = 'agents'
const chat = 'agents-chat'
const thread = `${chat}-thread`
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const agentRow = (page: Page) => frame(page).getByTestId('agents-agent-scripted')
const chats = (page: Page) => frame(page).getByTestId('agents-chats')
/** The chats in the list named `title`. */
const chatTitled = (page: Page, title: string) => chats(page).getByText(title, { exact: true })

async function openAgents(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  // the list has loaded (the window may open on the chat it had open)
  await expect(frame(page).getByTestId('agents-app')).toHaveAttribute('aria-busy', 'false')
}

/** In one column (a small window, a phone) the window may open on the remembered chat: back to the list. */
async function toList(page: Page): Promise<void> {
  await expect(frame(page).getByTestId('agents-app')).toHaveAttribute('aria-busy', 'false')
  const back = frame(page).getByTestId('agents-back')
  if (await back.isVisible()) await back.click()
}

async function newChat(page: Page): Promise<void> {
  await toList(page)
  await agentRow(page).click()
  // the window reopens on the last chat (it is remembered): wait until the new, empty one has replaced it
  await expect(frame(page).locator(`[data-testid^="${thread}-user-"]`)).toHaveCount(0)
  await expect(frame(page).getByTestId(`${chat}-composer-input`)).toBeEnabled()
}

async function say(page: Page, text: string): Promise<void> {
  const input = frame(page).getByTestId(`${chat}-composer-input`)
  await input.fill(text)
  await input.press('Enter')
}

test.describe.configure({ mode: 'serial' })

test.describe('Agents journeys', () => {
  test('AG1: open Agents from the Dock and see who is there', async ({ page }) => {
    await openDesktop(page)
    await page.getByTestId('dock-agents').click()
    await expect(windowOf(page, ID)).toBeVisible()
    await toList(page)
    await expect(agentRow(page)).toBeVisible()
    // a kit row draws its text in its own shadow root, which getByText reads (a framework may set it as a property)
    await expect(agentRow(page).getByText('Scripted agent', { exact: true })).toBeVisible()
    await expect(agentRow(page).getByText('Answers by script, for tests')).toBeVisible()
    // the desktop widget lists the same agents
    await page.getByTestId('dock-discover').click()
    await expect(page.getByTestId('widget-agent-activity-agent-scripted')).toBeVisible()
  })

  test('AG2: start a chat and talk to an agent; the first message names the chat', async ({
    page
  }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'hello')
    await expect(frame(page).getByTestId(thread)).toContainText('Hi there')
    await expect(chatTitled(page, 'hello').first()).toBeVisible()
    await say(page, 'echo this back')
    await expect(frame(page).getByTestId(thread)).toContainText('echo this back')
    await expect(chatTitled(page, 'echo this back')).toHaveCount(0) // only the first names it
  })

  test('AG3: the agent asks permission; the answer resumes the turn', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    // the agent numbers its prompts (perm-0, perm-1, ...) for as long as the host runs: match on the shape, not the number
    const prompt = frame(page).getByTestId(
      new RegExp(`^${thread}-permission-permission-perm-\\d+$`)
    )
    const allow = frame(page).getByTestId(
      new RegExp(`^${thread}-permission-permission-perm-\\d+-allow$`)
    )
    const deny = frame(page).getByTestId(
      new RegExp(`^${thread}-permission-permission-perm-\\d+-deny$`)
    )
    await say(page, 'ask')
    await expect(prompt).toBeVisible()
    await expect(deny).toBeVisible()
    // a reload does not lose an unanswered prompt (the window reopens on the chat it had open)
    await openAgents(page)
    await expect(allow).toBeVisible()
    await allow.click()
    await expect(frame(page).getByTestId(thread)).toContainText('Answer: selected')
    // and an answered prompt does not come back
    await openAgents(page)
    await expect(frame(page).getByTestId(thread)).toContainText('Answer: selected')
    await expect(allow).toHaveCount(0)
  })

  test('AG4: the session bar follows what the agent reports', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    const f = frame(page)
    await expect(f.getByTestId(`${chat}-session-mode`)).toBeAttached()
    await say(page, 'state')
    await expect(f.getByTestId(`${chat}-session-usage`)).toBeVisible()
    await expect(f.getByTestId(`${chat}-session-commands`)).toBeAttached()
    await expect(chatTitled(page, 'state').first()).toBeVisible() // the app titles the chat from the first message
    // the agent moved itself to Plan mode and the bar followed; the person can change mode and model too
    await expect(f.getByTestId(`${chat}-session-mode-value`)).toHaveText('Plan')
    await f.getByTestId(`${chat}-session-mode`).selectOption('default')
    await expect(f.getByTestId(`${chat}-session-mode-value`)).toHaveText('Default')
    await expect(f.getByTestId(`${chat}-session-config-model-value`)).toHaveText('Opus')
    await f.getByTestId(`${chat}-session-config-model`).selectOption('sonnet')
    await expect(f.getByTestId(`${chat}-session-config-model-value`)).toHaveText('Sonnet')
  })

  test('AG5: stop an agent that is still answering', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    const f = frame(page)
    await say(page, 'slow')
    await expect(f.getByTestId(`${chat}-composer-stop`)).toBeVisible()
    await f.getByTestId(`${chat}-composer-stop`).click()
    await expect(f.getByTestId(`${chat}-composer-stop`)).toHaveCount(0)
    await expect(f.getByTestId(`${chat}-composer-send`)).toBeVisible()
  })

  test('AG6: close the window and come back: the chat and its transcript are still there', async ({
    page
  }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'hello again')
    await expect(frame(page).getByTestId(thread)).toContainText('echo: hello again')
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await page.getByTestId('dock-agents').click()
    await expect(frame(page).getByTestId(thread)).toContainText('hello again')
    await expect(frame(page).getByTestId(thread)).toContainText('echo: hello again')
  })

  test('AG7: the Agents window at its smallest still works', async ({ page }) => {
    await openAgents(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width').toBe(480)
    expect(Math.round(win.height), 'the minimum height').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // below 700 wide the list and the chat are one column each: starting a chat opens it, Back returns to the list
    await newChat(page)
    await say(page, 'hello')
    await expect(frame(page).getByTestId(thread)).toContainText('Hi there')
    await expect(frame(page).getByTestId(`${chat}-composer-input`)).toBeInViewport()
    await frame(page).getByTestId('agents-back').click()
    await expect(chatTitled(page, 'hello').first()).toBeVisible()
    await expect(agentRow(page)).toBeVisible()
  })
})
