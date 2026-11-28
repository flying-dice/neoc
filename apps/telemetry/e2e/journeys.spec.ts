/**
 * Telemetry key user journeys, end to end: the real shell and host, the app's client in a frame of its window, over what
 * the host recorded of its own probes. Telemetry is not installed at start (the other journeys never open it): the
 * journeys install it, as a person does from Discover, and remove it when they are done. They share one install, so
 * they run one after another.
 */
import type { FrameLocator, Page } from '@playwright/test'
import { TELEMETRY_APP_REF } from '../../../e2e/host/config'
import { expect, expectRunning, frameOf, isRunning, openDesktop, openFromHome, test } from '../../../e2e/fixtures'

test.describe.configure({ mode: 'serial' })

const ID = 'telemetry'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string) => frame(page).getByTestId(`${ID}-${name}`)

/** The host-management call the shell makes (`window.__vos.host.api`), as the Get button of Discover does. */
const hostCall = <T>(page: Page, method: string, params: Record<string, unknown>): Promise<T> =>
  page.evaluate(
    ([m, p]) =>
      (window as unknown as { __vos: { host: { api: { call(method: string, params?: unknown): Promise<unknown> } } } }).__vos.host.api.call(m as string, p),
    [method, params] as const
  ) as Promise<T>

async function openTelemetry(page: Page): Promise<void> {
  // the app's frame writes to the clipboard, and the journey reads it back
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write'])
  await openDesktop(page)
  if (!(await isRunning(page, ID))) {
    const preview = await hostCall<{ sha256: string; permissions: string[] }>(page, 'app.preview', { url: TELEMETRY_APP_REF })
    expect(preview.permissions, 'the app asks for the clipboard, and nothing else').toEqual(['clipboard-write'])
    await hostCall(page, 'app.install', { url: TELEMETRY_APP_REF, sha256: preview.sha256 })
    await expectRunning(page, ID)
  }
  await openFromHome(page, ID)
  await expect(part(page, 'app')).toBeVisible()
}

/** What the clipboard holds, read by the page the frame sits in. */
const clipboard = (page: Page): Promise<string> => page.evaluate(() => navigator.clipboard.readText())

test.afterAll(async ({ browser }) => {
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openDesktop(page)
  if (await isRunning(page, ID)) {
    await hostCall(page, 'app.uninstall', { app: ID, purge: true })
    await expectRunning(page, ID, false)
  }
  await context.close()
})

test.describe('Telemetry journeys', () => {
  test('TE1: copy everything about a log, with its span and its trace, and about the trace', async ({ page }) => {
    await openTelemetry(page)
    // a call whose backend fails: its trace and its error log are what the host recorded
    await part(page, 'probe-fail').click()
    await part(page, 'view-logs').click()
    const failing = frame(page).getByTestId(/^telemetry-log-\d+$/).filter({ hasText: 'probe failing on purpose' })
    await expect(failing.first()).toBeVisible({ timeout: 30_000 })
    await failing.first().click()
    await expect(part(page, 'log-copy-status')).toHaveText('')
    await part(page, 'log-copy').click()
    await expect(part(page, 'log-copy-status')).toContainText('on the clipboard')
    const log = JSON.parse(await clipboard(page)) as {
      kind: string
      log: { message: string; severity: string; traceId: string; spanId: string; app: string; attributes: Record<string, unknown> }
      span: { name: string; source: string } | null
      trace: { traceId: string; spans: { name: string; source: string }[]; logs: { message: string }[] }
    }
    expect(log.kind).toBe('log')
    expect(log.log.message).toBe('probe failing on purpose')
    expect(log.log.severity).toBe('error')
    expect(log.log.app).toBe('telemetry')
    // the span it was written in, and the whole trace as the host has it (the page sends its own spans every 2 s)
    expect(log.span?.name).toBe('count units')
    expect(log.trace.traceId).toBe(log.log.traceId)
    expect(log.trace.spans.map((s) => s.source)).toEqual(expect.arrayContaining(['host', 'backend']))
    expect(log.trace.logs.map((l) => l.message)).toContain('probe failing on purpose')

    // from the log to its trace, and the trace's own copy
    await part(page, 'log-trace').click()
    await expect(part(page, 'trace-id')).toHaveText(log.log.traceId)
    await expect(part(page, 'trace-copy')).toBeEnabled()
    await part(page, 'trace-copy').click()
    await expect(part(page, 'trace-copy-status')).toContainText('on the clipboard')
    const trace = JSON.parse(await clipboard(page)) as { kind: string; traceId: string; spans: { name: string }[]; logs: { message: string }[] }
    expect(trace.kind).toBe('trace')
    expect(trace.traceId).toBe(log.log.traceId)
    expect(trace.spans.map((s) => s.name)).toContain('count units')
    expect(trace.logs.map((l) => l.message)).toContain('probe failing on purpose')
  })
})
