/**
 * Services key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend, which manages services through `vnd:service_manager`. The manager it reaches is a mock one: the
 * host of these specs is started with `NEOC_SERVICE_MANAGER=mock` (e2e/host/start.ts), so no journey touches a service
 * of whoever runs them, and they run the same on a machine whose service manager is systemd, launchd or none.
 *
 * The mock has services that "other programs" registered (`mock-nginx`, `mock-cups`, `mock-locked`, which the host may not
 * change, and `mock-broken`, which cannot be started). The journeys run one after another on the shared host, and each
 * leaves what it registered gone.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'services'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const item = (page: Page, label: string, what: string): Locator => part(page, `item-${label}-${what}`)
/** A program that is there on every platform, for a service that is registered and never run. */
const PROGRAM = process.platform === 'win32' ? 'C:\\Windows\\System32\\cmd.exe' : '/bin/sh'

test.describe.configure({ mode: 'serial' })

/** Register a service and wait until the form has been answered. */
async function register(page: Page, label: string, program: string): Promise<void> {
  await part(page, 'register').click()
  await field(page, 'form-label').fill(label)
  await field(page, 'form-program').fill(program)
  await part(page, 'form-register').click()
  await expect(part(page, 'form-busy')).toHaveCount(0)
}

/** Unregister the service that is looked at, answering the question. */
async function unregister(page: Page): Promise<void> {
  await part(page, 'detail-remove').click()
  await part(page, 'confirm-confirm').click()
  await expect(part(page, 'confirm')).toHaveCount(0)
}

async function openServices(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'summary')).toBeVisible()
}

test.describe('Services journeys', () => {
  test('SV1: open Services from the home screen and see what other programs registered', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(part(page, 'app')).toBeVisible()
    await expect(item(page, 'mock-nginx', 'open')).toContainText('Running')
    await expect(item(page, 'mock-cups', 'open')).toContainText('Stopped')
    await expect(item(page, 'mock-broken', 'open')).toContainText('Failed')
    // a filter shows the services in one state, and the count says how many
    await part(page, 'filter-failed').click()
    await expect(part(page, 'items').locator('li')).toHaveCount(1)
    await expect(item(page, 'mock-broken', 'open')).toBeVisible()
    await part(page, 'filter-all').click()
    // the search narrows the list
    await field(page, 'search').fill('print')
    await expect(part(page, 'items').locator('li')).toHaveCount(1)
    await expect(item(page, 'mock-cups', 'open')).toBeVisible()
    await field(page, 'search').fill('')
  })

  test('SV2: register a service, start it, look at it, stop it and unregister it', async ({ page }) => {
    await openServices(page)
    await part(page, 'register').click()
    await field(page, 'form-label').fill('e2e-app')
    await field(page, 'form-program').fill(PROGRAM)
    await part(page, 'form-environment').locator('textarea').fill('GREETING=hello world\nMODE=e2e')
    await field(page, 'form-restart-delay').fill('3')
    await part(page, 'form-register').click()
    await expect(part(page, 'form-busy')).toHaveCount(0)
    // registered and looked at, and not started
    await expect(part(page, 'detail-name')).toHaveText('e2e-app')
    // what was typed is what the manager has
    await expect(part(page, 'detail-environment')).toHaveJSProperty('value', 'GREETING=hello world MODE=e2e')
    await expect(part(page, 'detail-state')).toContainText('Stopped')
    await expect(part(page, 'detail-program')).toHaveJSProperty('value', PROGRAM)

    await part(page, 'detail-start').click()
    await expect(part(page, 'detail-state')).toContainText('Running')
    await expect(item(page, 'e2e-app', 'open')).toContainText('Running')

    // it is the host's, not the page's: a window opened again reads the same
    await openDesktop(page, { open: [ID] })
    await item(page, 'e2e-app', 'open').click()
    await expect(part(page, 'detail-state')).toContainText('Running')

    await part(page, 'detail-stop').click()
    await expect(part(page, 'detail-state')).toContainText('Stopped')
    await unregister(page)
    await expect(item(page, 'e2e-app', 'open')).toHaveCount(0)
  })

  test('SV3: a service the operating system will not let the host change says so, and stays as it was', async ({ page }) => {
    await openServices(page)
    await item(page, 'mock-locked', 'open').click()
    await expect(part(page, 'detail-state')).toContainText('Running')
    await part(page, 'detail-stop').click()
    await expect(part(page, 'detail-notice')).toHaveAttribute('data-status', 'denied')
    await expect(part(page, 'detail-notice')).toContainText('administrator rights')
    await expect(part(page, 'detail-state')).toContainText('Running')
    await expect(part(page, 'failure')).toHaveCount(0)
    // it cannot be removed either, and the question was asked first
    await unregister(page)
    await expect(part(page, 'detail-notice')).toHaveAttribute('data-status', 'denied')
    await expect(item(page, 'mock-locked', 'open')).toBeVisible()
  })

  test('SV4: a service that cannot start says it failed', async ({ page }) => {
    await openServices(page)
    await item(page, 'mock-broken', 'open').click()
    await part(page, 'detail-start').click()
    await expect(part(page, 'detail-notice')).toHaveAttribute('data-status', 'failed')
    await expect(part(page, 'detail-state')).toContainText('Failed')
  })

  test('SV5: what cannot be registered is refused at the form, which keeps what was typed', async ({ page }) => {
    await openServices(page)
    await part(page, 'register').click()
    await expect(part(page, 'form-register')).toHaveJSProperty('disabled', true)
    await field(page, 'form-label').fill('e2e-bad')
    await field(page, 'form-program').fill('relative/program')
    await part(page, 'form-register').click()
    await expect(part(page, 'form')).toHaveJSProperty('footerTone', 'error')
    await expect(part(page, 'form')).toHaveJSProperty('footer', 'option "program" must be an absolute path, not "relative/program"')
    await expect(field(page, 'form-label')).toHaveValue('e2e-bad')
    await expect(item(page, 'e2e-bad', 'open')).toHaveCount(0)
    await part(page, 'form-cancel').click()
    await expect(part(page, 'form')).toHaveCount(0)
  })

  test('SV6: a setting is changed and the rest of the service is left as it was', async ({ page }) => {
    await openServices(page)
    await item(page, 'mock-cups', 'open').click()
    await expect(part(page, 'detail-autostart')).toHaveJSProperty('checked', false)
    await part(page, 'detail-autostart').click()
    await expect(part(page, 'detail-autostart')).toHaveJSProperty('checked', true)
    await expect(part(page, 'detail-program')).toHaveJSProperty('value', '/usr/sbin/mock-cups')
    await expect(part(page, 'detail-state')).toContainText('Stopped')
    // the host keeps it
    await openDesktop(page, { open: [ID] })
    await item(page, 'mock-cups', 'open').click()
    await expect(part(page, 'detail-autostart')).toHaveJSProperty('checked', true)
    await part(page, 'detail-autostart').click()
    await expect(part(page, 'detail-autostart')).toHaveJSProperty('checked', false)
  })

  test('SV7: an agent sees the services and how they are doing', async ({ page }) => {
    await openServices(page)
    const listed = JSON.parse(await callTool(`${ID}__list_services`, {})) as { manager: string; services: { label: string; state: string }[] }
    expect(listed.manager).toBe('mock')
    expect(listed.services.find((s) => s.label === 'mock-nginx')?.state).toBe('running')
    const failed = JSON.parse(await callTool(`${ID}__list_services`, { state: 'failed' })) as { services: { label: string }[] }
    expect(failed.services.map((s) => s.label)).toContain('mock-broken')
    expect(failed.services.map((s) => s.label)).not.toContain('mock-nginx')
  })

  test('SV8: the smallest window still shows the list, and a service one push away', async ({ page }) => {
    await openServices(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(360)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(480)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await item(page, 'mock-nginx', 'open').click()
    await expect(part(page, 'detail-name')).toHaveText('mock-nginx')
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await part(page, 'detail-back').click()
    await expect(part(page, 'items')).toBeVisible()
  })
})
