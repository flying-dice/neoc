/**
 * Agents chat journeys, end to end in web mode on the in-memory host with its
 * scripted agent: ACP session controls (mode / model / effort), and the single
 * per-turn activity line that replaces individual tool calls.
 */
import {
  chooseLayout,
  expect,
  expectNoOverflow,
  openDesktop,
  settle,
  test,
  windowOf
} from '../../e2e/fixtures'

interface HostCall {
  method: string
  args: unknown[]
}

const hostCalls = (page: import('@playwright/test').Page, method: string): Promise<HostCall[]> =>
  page.evaluate(
    (m) =>
      (
        window as unknown as { __agentos: { host: { calls: HostCall[] } } }
      ).__agentos.host.calls.filter((c) => c.method === m),
    method
  )

test.describe('Agents journeys', () => {
  test('A1: a new chat offers the agent’s mode, model and effort, and changes reach the session', async ({
    page
  }) => {
    await openDesktop(page, { open: ['agents'] })
    await page.getByTestId('agents-new-chat').click()
    await settle(page)

    const mode = page.getByTestId('agents-mode')
    const model = page.getByTestId('agents-config-model')
    const effort = page.getByTestId('agents-config-effort')
    await expect(mode).toHaveValue('default')
    await expect(model).toHaveValue('large')
    await expect(effort).toHaveValue('medium')

    await effort.selectOption('high')
    await expect(effort).toHaveValue('high')
    await expect(page.getByTestId('agents-config-effort-value')).toHaveText('High')
    expect((await hostCalls(page, 'agents.setConfig')).at(-1)?.args.slice(2)).toEqual([
      'effort',
      'high'
    ])

    await mode.selectOption('plan')
    await expect(mode).toHaveValue('plan')
    await expect(page.getByTestId('agents-mode-value')).toHaveText('Plan')
    expect((await hostCalls(page, 'agents.setMode')).at(-1)?.args.at(-1)).toBe('plan')
  })

  test('A2: an agent turn shows one activity line with counts, not a list of tools', async ({
    page
  }) => {
    await openDesktop(page, { open: ['agents'] })
    await page.getByTestId('agents-new-chat').click()
    await page.getByTestId('agents-suggestion-0').click()

    // The scripted agent uses the Calculator's tools; the chat summarises rather than listing them.
    await expect(windowOf(page, 'calculator')).toBeVisible()
    const activity = page.getByRole('status').filter({ hasText: /tool call/ })
    await expect(activity).toHaveCount(1)
    await expect(activity).toHaveText(/\d+ tool calls?/)
    await expect(page.getByText(/calculator__|Calculator · /)).toHaveCount(0)
    await expect(page.getByTestId('agents-stop')).toHaveCount(0)
  })

  test('A3: session controls stay inside a narrow Split View window', async ({ page }) => {
    await openDesktop(page, { open: ['agents'] })
    await page.getByTestId('agents-new-chat').click()
    await chooseLayout(page, 'agents', 'split-left')
    await settle(page)
    await expect(page.getByTestId('agents-config-effort')).toBeVisible()
    await expectNoOverflow(windowOf(page, 'agents'))
  })
})
