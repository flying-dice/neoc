/**
 * Keychain key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its
 * window, and its Luau backend, which keeps the passwords through `vnd:keyring`. The keychain it reaches is a mock
 * one: the host of these specs is started with `NEOC_KEYRING=mock` (e2e/host/start.ts), so no journey touches the
 * keychain of whoever runs them, and they run the same on a machine that has none.
 *
 * The app's list is kept on the shared host: the tests run one after another, each starting from no items.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'keychain'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const of = (page: Page, suffix: string): Locator => frame(page).locator(`[data-testid^="${ID}-item-"][data-testid$="-${suffix}"]`)
const services = (page: Page): Locator => of(page, 'service')

test.describe.configure({ mode: 'serial' })

/** Type an item and add it. */
async function add(page: Page, service: string, account: string, password: string): Promise<void> {
  await field(page, 'new-service').fill(service)
  await field(page, 'new-account').fill(account)
  await field(page, 'new-password').fill(password)
  await part(page, 'new-add').click()
}

/** Delete the first item, answering the question. */
async function removeFirst(page: Page): Promise<void> {
  await of(page, 'remove').first().click()
  await part(page, 'confirm-delete').click()
  await expect(part(page, 'confirm')).toHaveCount(0)
}

/** The app's window as a test starts: no items. */
async function openKeychain(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'items-summary')).toBeVisible()
  // what an earlier test left behind
  for (let left = await services(page).count(); left > 0; left -= 1) {
    await removeFirst(page)
    await expect(services(page)).toHaveCount(left - 1)
  }
  await expect(part(page, 'empty')).toBeVisible()
}

test.describe('Keychain journeys', () => {
  test('KC1: open Keychain from the home screen and keep a password', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(part(page, 'app')).toBeVisible()
    await openKeychain(page)
    // nothing to add yet: the button cannot be pressed
    await expect(part(page, 'new-add')).toHaveJSProperty('disabled', true)

    await add(page, 'router.home', 'admin', 'tr0ub4dor&3')
    await expect(part(page, 'items-summary')).toHaveText('1 item')
    await expect(services(page)).toHaveText(['router.home'])
    await expect(of(page, 'account')).toHaveText(['admin'])
    // the fields are empty for the next one, and the password is nowhere in the window
    await expect(field(page, 'new-service')).toHaveValue('')
    await expect(field(page, 'new-password')).toHaveValue('')
    await expect(of(page, 'password')).toHaveCount(0)
    await expect(part(page, 'app')).not.toContainText('tr0ub4dor&3')
  })

  test('KC2: a password is shown when it is asked for, and hidden again', async ({ page }) => {
    await openKeychain(page)
    await add(page, 'router.home', 'admin', 'tr0ub4dor&3')
    await add(page, 'backup.example.com', 'ada', 'correct horse battery staple')
    await expect(services(page)).toHaveText(['backup.example.com', 'router.home'])

    await of(page, 'reveal').nth(1).click()
    await expect(of(page, 'password')).toHaveText(['tr0ub4dor&3'])
    await of(page, 'hide').click()
    await expect(of(page, 'password')).toHaveCount(0)

    // it is kept by the host, not by the page: a window opened again reads the same password
    await openDesktop(page, { open: [ID] })
    await expect(services(page)).toHaveText(['backup.example.com', 'router.home'])
    await of(page, 'reveal').first().click()
    await expect(of(page, 'password')).toHaveText(['correct horse battery staple'])
  })

  test('KC3: an item that is there is not added again: the form says so, the window does not', async ({ page }) => {
    await openKeychain(page)
    await add(page, 'router.home', 'admin', 'tr0ub4dor&3')
    await expect(part(page, 'items-summary')).toHaveText('1 item')
    await add(page, 'router.home', 'admin', 'another')
    await expect(part(page, 'new-form')).toHaveJSProperty('footer', 'This item is here already.')
    await expect(part(page, 'new-form')).toHaveJSProperty('footerTone', 'error')
    await expect(part(page, 'failure')).toHaveCount(0)
    await expect(part(page, 'items-summary')).toHaveText('1 item')
    // the refusal goes when the item is typed again, and the first password is the one still kept
    await field(page, 'new-account').fill('root')
    await expect(part(page, 'new-form')).toHaveJSProperty('footerTone', 'note')
    await of(page, 'reveal').click()
    await expect(of(page, 'password')).toHaveText(['tr0ub4dor&3'])
  })

  test('KC4: deleting an item asks first, and takes it from the keychain', async ({ page }) => {
    await openKeychain(page)
    await add(page, 'router.home', 'admin', 'tr0ub4dor&3')
    await of(page, 'remove').click()
    await expect(part(page, 'confirm')).toContainText('Delete router.home?')
    await part(page, 'confirm-cancel').click()
    await expect(part(page, 'confirm')).toHaveCount(0)
    await expect(services(page)).toHaveText(['router.home'])

    await removeFirst(page)
    await expect(part(page, 'empty')).toBeVisible()
    await expect(part(page, 'items-summary')).toHaveText('No items')
    // it is gone from the keychain too: with no password typed there is nothing to show
    await add(page, 'router.home', 'admin', '')
    await expect(part(page, 'new-form')).toHaveJSProperty('footer', 'The keychain has no password for this service and account. Type one to keep it.')
    await expect(part(page, 'empty')).toBeVisible()
  })

  test('KC5: an agent keeps a password and sees the items, and is never given a password', async ({ page }) => {
    await openKeychain(page)
    await callTool(`${ID}__add_keychain_item`, { service: 'ghcr.io', account: 'ada-lovelace', password: 'ghp_x7Kq2mVn9RtL4wZs' })
    // the open window hears of it
    await expect(services(page)).toHaveText(['ghcr.io'])
    await of(page, 'reveal').click()
    await expect(of(page, 'password')).toHaveText(['ghp_x7Kq2mVn9RtL4wZs'])

    const listed = await callTool(`${ID}__list_keychain_items`, {})
    expect(JSON.parse(listed)).toEqual({ items: [{ service: 'ghcr.io', account: 'ada-lovelace' }] })
    expect(listed).not.toContain('ghp_')
    // what the app refuses a person it refuses an agent, in the same words
    await expect(callTool(`${ID}__add_keychain_item`, { service: 'ghcr.io', account: 'ada-lovelace', password: 'another' })).rejects.toThrow(
      'This item is here already.'
    )
  })

  test('KC6: the smallest window still shows the form and the items', async ({ page }) => {
    await openKeychain(page)
    await add(page, 'a-rather-long-service-name.internal.example.com', 'someone-with-a-long-name@example.com', 'secret')
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(360)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await of(page, 'reveal').click()
    await expect(of(page, 'password')).toHaveText(['secret'])
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await removeFirst(page)
  })
})
