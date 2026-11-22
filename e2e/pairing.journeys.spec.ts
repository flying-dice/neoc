/**
 * Pairing journeys: a browser the host does not know yet sees the pairing screen,
 * says plainly when the code is wrong, and gets the desktop when the code is right. These start unpaired.
 * (Rate limiting is covered by the host's pairing tests and the "Too many attempts" story: exhausting the
 * attempt budget here would lock out every other spec's pairing for a minute.)
 */
import { expect, openDesktop, test } from './fixtures'
import { issuePairingCode } from './host/pair'

test.use({ storageState: { cookies: [], origins: [] } })

test.describe('Pairing journeys', () => {
  test('P1: an unpaired visitor sees the pairing screen, not the desktop', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('pairing')).toBeVisible()
    await expect(page.getByTestId('pairing-submit')).toBeDisabled()
    await expect(page.getByTestId('dock')).toHaveCount(0)
  })

  test('P2: a wrong code is refused with a clear message and the visitor can try again', async ({ page }) => {
    await page.goto('/')
    await page.getByTestId('pairing-code').fill('ZZZZ-ZZZZ')
    await page.getByTestId('pairing-submit').click()
    await expect(page.getByTestId('pairing-error')).toContainText('did not work')
    await expect(page.getByTestId('pairing-error')).toContainText('10 minutes')
    await expect(page.getByTestId('dock')).toHaveCount(0)
    // typing again clears the message: the visitor is not stuck
    await page.getByTestId('pairing-code').fill('')
    await expect(page.getByTestId('pairing-error')).toHaveCount(0)
  })

  test('P3: the code the host prints pairs the device and the desktop appears', async ({ page }) => {
    const code = issuePairingCode()
    await page.goto('/')
    await page.getByTestId('pairing-code').fill(code.toLowerCase().replace('-', ' ')) // forgiving input
    await expect(page.getByTestId('pairing-code')).toHaveValue(code)
    await page.getByTestId('pairing-device-name').fill('Journey browser')
    await page.getByTestId('pairing-submit').click()
    await expect(page.getByTestId('dock')).toBeVisible()
    await expect(page.getByTestId('status-connection')).toHaveCount(0)

    // paired for good: a reload goes straight to the desktop
    await openDesktop(page)
    await expect(page.getByTestId('pairing')).toHaveCount(0)
  })

  test('P4: a code works once', async ({ page, browser }) => {
    const code = issuePairingCode()
    await page.goto('/')
    await page.getByTestId('pairing-code').fill(code)
    await page.getByTestId('pairing-submit').click()
    await expect(page.getByTestId('dock')).toBeVisible()

    const other = await browser.newPage({ storageState: { cookies: [], origins: [] } })
    await other.goto('/')
    await other.getByTestId('pairing-code').fill(code)
    await other.getByTestId('pairing-submit').click()
    await expect(other.getByTestId('pairing-error')).toContainText('already used')
    await other.close()
  })
})
