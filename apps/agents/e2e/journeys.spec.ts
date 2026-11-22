/**
 * Agents key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window, and
 * its Luau backend, which keeps the chats and asks the agent. The agent is the backend's scripted stand-in
 * (`../server/agent.luau`: the prompt text picks what it says), until an agent is a process the backend starts.
 *
 * The chats live on the shared host, so each test starts its own chat and finds it by what it said.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, resizeWindow, test, windowOf } from '../../../e2e/fixtures'

const ID = 'agents'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
const agentRow = (page: Page): Locator => part(page, 'agent-scripted')
const thread = (page: Page): Locator => part(page, 'thread')
// the kit's text field holds the real input in its shadow tree
const input = (page: Page): Locator => part(page, 'composer-input').locator('input')

async function openAgents(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await expect(agentRow(page)).toBeVisible()
}

async function newChat(page: Page): Promise<void> {
  await agentRow(page).click()
  await expect(input(page)).toBeVisible()
  await expect(frame(page).getByTestId(/^agents-message-/)).toHaveCount(0)
}

async function say(page: Page, text: string): Promise<void> {
  await input(page).fill(text)
  await input(page).press('Enter')
}

test.describe('Agents journeys', () => {
  test('AG1: open Agents from the Dock and see who is there', async ({ page }) => {
    await openDesktop(page)
    await page.getByTestId(`dock-${ID}`).click()
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(agentRow(page)).toContainText('Scripted agent')
    await expect(part(page, 'no-chat')).toBeVisible()
  })

  test('AG2: start a chat and talk to an agent; the first message names the chat', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
    await expect(part(page, 'title')).toHaveText('hello')
    await expect(part(page, 'chats')).toContainText('hello')
    await expect(part(page, 'composer-send')).toBeVisible() // the turn is over
    await say(page, 'echo this back')
    await expect(thread(page)).toContainText('echo: echo this back')
    await expect(part(page, 'title')).toHaveText('hello') // only the first message names it
  })

  test('AG3: stop an agent that is still answering; what it said stays', async ({ page }) => {
    await openAgents(page)
    await newChat(page)
    await say(page, 'slow')
    await expect(part(page, 'composer-stop')).toBeVisible()
    await expect(thread(page)).toContainText('1 ')
    await part(page, 'composer-stop').click()
    await expect(part(page, 'composer-stop')).toHaveCount(0)
    await expect(part(page, 'composer-send')).toBeVisible()
    await expect(thread(page)).toContainText('1 ')
    // and the chat takes the next message
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
  })

  test('AG4: close the window and come back: the chat and its transcript are still there', async ({ page }) => {
    const said = `remember ${Date.now()}`
    await openAgents(page)
    await newChat(page)
    await say(page, said)
    await expect(thread(page)).toContainText(`echo: ${said}`)
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await page.getByTestId(`dock-${ID}`).click()
    await frame(page).getByTestId(/^agents-chat-/).filter({ hasText: said }).click()
    await expect(thread(page)).toContainText(`echo: ${said}`)
  })

  test('AG5: the Agents window at its smallest still works', async ({ page }) => {
    await openAgents(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(480)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // below 700 wide the list and the chat are one column each: starting a chat opens it, Back returns to the list
    await newChat(page)
    await expect(agentRow(page)).toBeHidden()
    await say(page, 'hello')
    await expect(thread(page)).toContainText('Hi there')
    await expect(input(page)).toBeInViewport()
    await part(page, 'back').click()
    await expect(agentRow(page)).toBeVisible()
    await expect(part(page, 'chats')).toContainText('hello')
  })
})
