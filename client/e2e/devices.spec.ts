/**
 * Devices journeys (client/journeys/settings.md): Settings > Devices lists the browsers paired with the host,
 * marks the one in use, and revokes the others, which then land on the pairing screen.
 */
import { expect, openDesktop, test } from '../../e2e/fixtures'
import { pairDevice } from '../../e2e/host/pair'

async function openDevices(page: import('@playwright/test').Page): Promise<void> {
  await openDesktop(page, { open: ['settings'] })
  await page.getByTestId('settings-pane-devices').click()
  await expect(page.getByTestId('settings-devices')).toBeVisible()
}

test.describe('Devices journeys', () => {
  test('S8: see which devices are paired and which one is this', async ({ page }) => {
    await openDevices(page)
    const mine = page.locator('[data-testid^="settings-device-"]', { hasText: 'This device' })
    await expect(mine).toHaveCount(1)
    await expect(mine).toContainText('Playwright')
    // this device has no Revoke: revoking yourself only sends you back to the pairing screen
    await expect(mine.locator('[data-testid$="-revoke"]')).toHaveCount(0)
  })

  test('S9: revoke another device; it is signed out and must pair again', async ({
    page,
    browser
  }) => {
    // a second browser pairs and has the desktop open
    const other = await pairDevice('Second browser')
    const second = await browser.newContext({
      storageState: { cookies: [{ ...other.cookie, expires: -1, secure: false }], origins: [] }
    })
    const secondPage = await second.newPage()
    await secondPage.goto('/')
    await expect(secondPage.getByTestId('dock')).toBeVisible()

    // the first browser sees it and cancels the revoke once
    await openDevices(page)
    const row = page.getByTestId(`settings-device-${other.deviceId}`)
    await expect(row).toContainText('Second browser')
    await page.getByTestId(`settings-device-${other.deviceId}-revoke`).click()
    await expect(page.getByTestId('settings-device-revoke-alert')).toContainText('Second browser')
    await page.getByTestId('settings-device-revoke-alert-action-cancel').click()
    await expect(row).toBeVisible()

    // then confirms: the row goes and the second browser lands on the pairing screen
    await page.getByTestId(`settings-device-${other.deviceId}-revoke`).click()
    await page.getByTestId('settings-device-revoke-alert-action-revoke').click()
    await expect(row).toHaveCount(0)
    await expect(secondPage.getByTestId('pairing')).toBeVisible({ timeout: 20_000 })

    // and its old cookie no longer opens anything
    const session = await fetch(`${page.url().split('/').slice(0, 3).join('/')}/session`, {
      headers: { cookie: `agentos_session=${other.cookie.value}` }
    })
    expect(await session.json()).toEqual({ paired: false })
    await second.close()
  })
})
