/**
 * Agents key user journeys (../JOURNEYS.md), end to end: the real shell, the real host, the Luau Agents app, and an
 * ACP agent on the host's roster that is a script, not a model (`crates/agentos-acp/src/bin/fake_agent.rs`; the
 * prompt text picks what it does). The conversation streams over AG-UI, the permission prompt is an AG-UI
 * interrupt, the Agents app only lists agents and chats.
 *
 * The chats live on the shared host, so each test starts its own chat and finds it by what it said.
 */
import type { Page } from '@playwright/test'
import { chooseLayout, expect, openDesktop, resizeWindow, test, windowOf } from '../../../e2e/fixtures'

const chat = 'agents-chat'
const thread = `${chat}-thread`
const agentRow = 'agents-sidebar-agents-agent-scripted-tap'

async function openAgents(page: Page): Promise<void> {
  await openDesktop(page, { open: ['agents'] })
  await expect(page.getByTestId('agents-view')).toBeVisible()
}

async function newChat(page: Page): Promise<void> {
  await page.getByTestId(agentRow).click()
  // the window reopens on the last chat (it is remembered): wait until the new, empty one has replaced it
  await expect(page.locator(`[data-testid^="${thread}-user-"]`)).toHaveCount(0)
  await expect(page.getByTestId(`${chat}-composer-input`)).toBeVisible()
}

async function say(page: Page, text: string): Promise<void> {
  const input = page.getByTestId(`${chat}-composer-input`)
  await input.fill(text)
  await input.press('Enter')
}

test.describe('Agents journeys', () => {
  test('AG1: open Agents from the Dock and see who is there', async ({ page }) => {
    await openDesktop(page)
    await page.getByTestId('dock-agents').click()
    await expect(windowOf(page, 'agents')).toBeVisible()
    await expect(page.getByTestId('agents-view')).toBeVisible()
    await expect(page.getByTestId('agents-sidebar-agents-agent-scripted')).toContainText('Scripted agent')
    await expect(page.getByTestId('agents-no-chat')).toBeVisible()
    // the desktop widget lists the same agents
    await page.getByTestId('dock-discover').click()
    await expect(page.getByTestId('widget-agent-activity-agent-scripted')).toBeVisible()
  })

  test('AG2: start a chat and talk to an agent; the first message names the chat', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'hello')
    await expect(page.getByTestId(thread)).toContainText('Hi there')
    await expect(page.getByTestId('agents-sidebar-chats')).toContainText('hello')
    await say(page, 'echo this back')
    await expect(page.getByTestId(thread)).toContainText('echo this back')
    await expect(page.getByTestId('agents-sidebar-chats')).not.toContainText('echo this back') // only the first names it
  })

  test('AG3: the agent asks permission; the answer resumes the turn', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'ask')
    const prompt = page.getByTestId(`${thread}-permission-permission-perm-0`)
    await expect(prompt).toBeVisible()
    await prompt.getByTestId(`${thread}-permission-permission-perm-0-allow`).click()
    await expect(page.getByTestId(thread)).toContainText('Answer: selected')
    // a reload re-offers an unanswered prompt, and answered ones stay answered
    await openAgents(page)
    await expect(page.getByTestId(`${thread}-permission-permission-perm-0-allow`)).toHaveCount(0)
  })

  test('AG4: the session bar follows what the agent reports', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    await expect(page.getByTestId(`${chat}-session-mode`)).toBeVisible()
    await say(page, 'state')
    await expect(page.getByTestId(`${chat}-session-usage`)).toBeVisible()
    await expect(page.getByTestId(`${chat}-session-commands`)).toBeVisible()
    await expect(page.getByTestId('agents-sidebar-chats')).toContainText('state') // the host's own title is the agent's, the app's is the first message
  })

  test('AG5: stop an agent that is still answering', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'slow')
    await expect(page.getByTestId(`${chat}-composer-stop`)).toBeVisible()
    await page.getByTestId(`${chat}-composer-stop`).click()
    await expect(page.getByTestId(`${chat}-composer-stop`)).toHaveCount(0)
    await expect(page.getByTestId(`${chat}-composer-send`)).toBeVisible()
  })

  test('AG6: close the window and come back: the chat and its transcript are still there', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'hello again')
    await expect(page.getByTestId(thread)).toContainText('echo: hello again')
    await chooseLayout(page, 'agents', 'close')
    await expect(windowOf(page, 'agents')).toHaveCount(0)
    await page.getByTestId('dock-agents').click()
    await expect(page.getByTestId(thread)).toContainText('hello again')
    await expect(page.getByTestId(thread)).toContainText('echo: hello again')
  })

  test('AG7: the Agents window at its smallest still works', async ({ page }) => {
    await openAgents(page)
    await resizeWindow(page, 'agents', 100, 100)
    const win = (await windowOf(page, 'agents').boundingBox())!
    expect(Math.round(win.width)).toBe(480)
    expect(Math.round(win.height)).toBe(420)
    await expect(page.getByTestId('agents-view')).toBeVisible()
  })
})
