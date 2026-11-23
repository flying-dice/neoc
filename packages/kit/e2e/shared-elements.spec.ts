/**
 * The alert, the split view, the text area and the picker in an app frame (sandboxed, opaque origin, the frame's policy),
 * driven by a real keyboard and a real window size: what a story's simulated events cannot show.
 */
import { startFrameServer, type FrameServer } from './frame-server'
import type { Page } from '@playwright/test'
import { expect, test } from '../../../e2e/fixtures'

let server: FrameServer

test.beforeAll(async () => {
  server = await startFrameServer()
})
test.afterAll(() => server.close())

/** An element to build in the page: its tag, its attributes, and its text or the elements in it. */
interface Built {
  tag: string
  attrs?: Record<string, string>
  text?: string
  children?: Built[]
}
const el = (
  tag: string,
  attrs: Record<string, string> = {},
  inside: string | Built[] = []
): Built =>
  typeof inside === 'string' ? { tag, attrs, text: inside } : { tag, attrs, children: inside }

/** Replaces the page's body with `built` and keeps every event named in `events` of the element `[data-testid=id]`. */
const stage = (page: Page, built: Built[], id: string, events: string[]): Promise<void> =>
  page.evaluate(
    ({ built, id, events }) => {
      const w = window as unknown as { heard: string[] }
      w.heard = []
      const make = (b: Built): HTMLElement => {
        const node = document.createElement(b.tag)
        for (const [name, value] of Object.entries(b.attrs ?? {})) node.setAttribute(name, value)
        if (b.text !== undefined) node.textContent = b.text
        node.append(...(b.children ?? []).map(make))
        return node
      }
      document.body.replaceChildren(...built.map(make))
      const target = document.querySelector(`[data-testid=${id}]`)
      for (const type of events) target?.addEventListener(type, () => w.heard.push(type))
    },
    { built, id, events }
  )
const heard = (page: Page): Promise<string[]> =>
  page.evaluate(() => (window as unknown as { heard: string[] }).heard)

test('an alert keeps the keyboard inside and the page behind it still, answers Cancel to Escape every time, and gives the focus back', async ({
  page
}) => {
  await page.goto(`${server.origin}/frame`)
  await stage(
    page,
    [
      el('ui-button', { 'data-testid': 'ask' }, 'Delete List'),
      el('ui-button', { 'data-testid': 'behind' }, 'Behind'),
      el('ui-alert', {
        'data-testid': 'alert',
        destructive: '',
        heading: 'Delete Work?',
        message: 'Its todos are deleted with it.',
        'action-label': 'Delete'
      })
    ],
    'alert',
    ['confirm', 'cancel']
  )
  await page.evaluate(() => {
    const alert = document.querySelector('ui-alert') as HTMLElement & { open: boolean }
    document
      .querySelector('[data-testid=ask]')
      ?.addEventListener('click', () => (alert.open = true))
    document
      .querySelector('[data-testid=behind]')
      ?.addEventListener('click', () =>
        (window as unknown as { heard: string[] }).heard.push('behind')
      )
  })
  const alert = page.getByTestId('alert')
  const dialog = alert.getByRole('alertdialog', { name: 'Delete Work?' })
  const cancel = page.getByTestId('alert-cancel')
  const confirm = page.getByTestId('alert-confirm')

  // three times over: the browser closes a dialog itself when Escape is pressed twice with nothing between
  for (let round = 0; round < 3; round++) {
    await page.getByTestId('ask').focus()
    await page.keyboard.press('Enter')
    await expect(dialog).toBeVisible()
    await expect(dialog).toHaveAccessibleDescription('Its todos are deleted with it.')
    // the answer that deletes nothing has the focus, and Tab goes round the two answers only
    await expect(cancel).toBeFocused()
    await page.keyboard.press('Tab')
    await expect(confirm).toBeFocused()
    await page.keyboard.press('Tab')
    await expect(confirm).not.toBeFocused()
    await page.keyboard.press('Shift+Tab')
    await expect(page.getByTestId('behind')).not.toBeFocused()
    // the page behind is inert: a click on it reaches nothing
    await page
      .getByTestId('behind')
      .click({ force: true, timeout: 2000 })
      .catch(() => undefined)
    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
    await expect(page.getByTestId('ask')).toBeFocused()
  }
  expect(await heard(page)).toEqual(['cancel', 'cancel', 'cancel'])

  // Return on the answer in front is Cancel; the other answer goes ahead, once
  await page.keyboard.press('Enter')
  await expect(cancel).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(dialog).toBeHidden()
  await page.keyboard.press('Enter')
  await expect(cancel).toBeFocused()
  await page.keyboard.press('Tab')
  await page.keyboard.press('Enter')
  await expect(dialog).toBeHidden()
  await expect(page.getByTestId('ask')).toBeFocused()
  expect(await heard(page)).toEqual(['cancel', 'cancel', 'cancel', 'cancel', 'confirm'])
  expect(
    await page.evaluate(() => (window as unknown as { violations: string[] }).violations)
  ).toEqual([])
})

test('a split view shows one column under 700 wide and every column from 700, whatever the app', async ({
  page
}) => {
  await page.setViewportSize({ width: 900, height: 500 })
  await page.goto(`${server.origin}/frame`)
  await stage(
    page,
    [
      el('ui-split-view', { 'data-testid': 'split', style: 'height:300px' }, [
        el('div', { slot: 'sidebar', 'data-testid': 'sidebar' }, 'Sidebar'),
        el('div', { 'data-testid': 'detail' }, 'Detail')
      ])
    ],
    'split',
    ['collapsedchange']
  )
  await page.evaluate(() => (document.body.style.margin = '0'))
  const split = page.getByTestId('split')
  await expect(page.getByTestId('sidebar')).toBeVisible()
  await expect(page.getByTestId('detail')).toBeVisible()

  await page.setViewportSize({ width: 699, height: 500 })
  await expect(split).toHaveAttribute('collapsed')
  await expect(page.getByTestId('sidebar')).toBeVisible()
  await expect(page.getByTestId('detail')).toBeHidden()
  await split.evaluate((el) => ((el as HTMLElement & { column: string }).column = 'detail'))
  await expect(page.getByTestId('sidebar')).toBeHidden()
  await expect(page.getByTestId('detail')).toBeVisible()

  await page.setViewportSize({ width: 700, height: 500 })
  await expect(split).not.toHaveAttribute('collapsed')
  await expect(page.getByTestId('sidebar')).toBeVisible()
  await expect(page.getByTestId('detail')).toBeVisible()
  expect(await heard(page)).toEqual(['collapsedchange', 'collapsedchange'])
})

test('a text area takes lines without submitting its form, and a picker takes a choice; both tell the form', async ({
  page
}) => {
  await page.goto(`${server.origin}/frame`)
  await stage(
    page,
    [
      el('form', { 'data-testid': 'form' }, [
        el('ui-text-area', { 'data-testid': 'area', name: 'notes', label: 'Notes', required: '' }),
        el('ui-picker', { 'data-testid': 'picker', name: 'folder', label: 'Folder', value: '1' })
      ])
    ],
    'form',
    ['submit', 'input', 'change']
  )
  await page.evaluate(() => {
    const picker = document.querySelector('ui-picker') as HTMLElement & { options: unknown }
    picker.options = [
      { value: '1', label: 'Notes' },
      { value: '2', label: 'Work' }
    ]
    document.querySelector('form')?.addEventListener('submit', (e) => e.preventDefault())
  })
  const entries = (): Promise<[string, FormDataEntryValue][]> =>
    page.evaluate(() => [...new FormData(document.querySelector('form') as HTMLFormElement)])
  const valid = (): Promise<boolean> =>
    page.evaluate(() => (document.querySelector('form') as HTMLFormElement).checkValidity())

  expect(await valid()).toBe(false) // the text is required
  await page.getByTestId('area').focus()
  await page.keyboard.type('Milk')
  await page.keyboard.press('Enter')
  await page.keyboard.type('Bread')
  await page.keyboard.press('Tab') // leaving the field commits the text: change
  await expect(page.getByTestId('picker').locator('select')).toBeFocused()
  expect(await valid()).toBe(true)
  expect(await entries()).toEqual([
    ['notes', 'Milk\nBread'],
    ['folder', '1']
  ])
  const before = await heard(page)
  expect(before).not.toContain('submit')
  expect(before.filter((type) => type === 'change')).toEqual(['change'])

  await page.getByTestId('picker').locator('select').selectOption({ label: 'Work' })
  expect(await entries()).toEqual([
    ['notes', 'Milk\nBread'],
    ['folder', '2']
  ])
  expect((await heard(page)).filter((type) => type === 'change')).toEqual(['change', 'change'])
})
