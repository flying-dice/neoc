/**
 * The built kit (`client/public/kit/kit.js` and `tokens.css`) under the conditions of an app frame: a sandboxed,
 * opaque-origin document with the frame's content security policy, which loads the two files as classic same-host
 * scripts. No host is needed: a tiny server stands in for it, so this checks the kit itself (that it runs under the
 * policy, defines every tag, follows `data-theme`) and not the host's wiring, which the app journeys check.
 */
import { readFileSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { join } from 'node:path'
import { expect, test } from '../../../e2e/fixtures'

const KIT = join(import.meta.dirname, '../../../client/public/kit')
/** The frame's policy, as the host sends it (NEOC-300). */
const CSP = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; sandbox allow-scripts"

/** The same without the sandbox: a standalone app is an ordinary full-screen page. */
const CSP_STANDALONE = CSP.replace('; sandbox allow-scripts', '')

const BODY = `<head>
<meta charset="utf-8"><title>Kit frame</title>
<link rel="stylesheet" href="/kit/tokens.css">
<script src="/kit/kit.js"></script>
</head>
<body>
<main style="padding:16px;display:grid;gap:12px;width:360px">
  <ui-button data-testid="button">Continue</ui-button>
  <ui-toggle data-testid="toggle" label="Sync"></ui-toggle>
  <ui-segmented data-testid="segmented" label="Filter" value="a"></ui-segmented>
  <ui-text-field data-testid="field" label="Name"></ui-text-field>
</main>
<script src="/page.js"></script>
</body>
</html>`

/** Embedded in a shell window: the host sets data-theme and color-scheme on the document. */
const PAGE = `<!doctype html>\n<html data-theme="light" style="color-scheme: light">\n${BODY}`
/** Standalone (`/apps/<id>` opened on its own): no shell, so nothing sets data-theme. */
const STANDALONE = `<!doctype html>\n<html>\n${BODY}`

const PAGE_JS = `
document.querySelector('ui-segmented').options = [{ value: 'a', label: 'A' }, { value: 'b', label: 'B' }]
window.violations = []
document.addEventListener('securitypolicyviolation', (e) => window.violations.push(e.violatedDirective + ' ' + e.blockedURI))
`

let server: Server
let origin: string

test.beforeAll(async () => {
  server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://x')
    if (url.pathname === '/frame' || url.pathname === '/standalone') {
      const framed = url.pathname === '/frame'
      res
        .writeHead(200, { 'content-type': 'text/html', 'content-security-policy': framed ? CSP : CSP_STANDALONE })
        .end(framed ? PAGE : STANDALONE)
    } else if (url.pathname === '/page.js') {
      res.writeHead(200, { 'content-type': 'text/javascript' }).end(PAGE_JS)
    } else if (url.pathname === '/kit/kit.js' || url.pathname === '/kit/tokens.css') {
      const type = url.pathname.endsWith('.js') ? 'text/javascript' : 'text/css'
      res.writeHead(200, { 'content-type': type }).end(readFileSync(join(KIT, url.pathname.slice(5))))
    } else res.writeHead(404).end()
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
})
test.afterAll(() => server.close())

test('the kit runs in a sandboxed frame under the frame policy and answers the keyboard', async ({ page }) => {
  await page.goto(`${origin}/frame`)
  expect(await page.evaluate(() => location.origin)).toBe('null') // the frame has an opaque origin
  const tags = await page.evaluate(() =>
    ['ui-banner', 'ui-button', 'ui-check', 'ui-empty-state', 'ui-icon', 'ui-list', 'ui-row', 'ui-section', 'ui-segmented', 'ui-spinner', 'ui-text-field', 'ui-toggle'].filter(
      (tag) => !customElements.get(tag)
    )
  )
  expect(tags, 'tags the kit did not define').toEqual([])

  // a button presses by keyboard, a toggle flips, a segmented control moves, a field takes text
  await page.evaluate(() => {
    ;(window as unknown as { log: string[] }).log = []
    for (const id of ['button', 'toggle', 'segmented', 'field']) {
      for (const type of ['click', 'change']) {
        document.querySelector(`[data-testid=${id}]`)?.addEventListener(type, () => (window as unknown as { log: string[] }).log.push(`${id}:${type}`))
      }
    }
  })
  await page.getByTestId('button').focus()
  await page.keyboard.press('Enter')
  await page.keyboard.press('Tab')
  await page.keyboard.press('Space')
  await page.keyboard.press('Tab')
  await page.keyboard.press('ArrowRight')
  await page.keyboard.press('Tab')
  await page.keyboard.type('Ada')
  expect(await page.evaluate(() => (document.querySelector('[data-testid=toggle]') as HTMLElement & { checked: boolean }).checked)).toBe(true)
  expect(await page.evaluate(() => (document.querySelector('[data-testid=segmented]') as HTMLElement & { value: string }).value)).toBe('b')
  expect(await page.evaluate(() => (document.querySelector('[data-testid=field]') as HTMLElement & { value: string }).value)).toBe('Ada')
  expect(await page.evaluate(() => (window as unknown as { log: string[] }).log)).toEqual(
    expect.arrayContaining(['button:click', 'toggle:change', 'segmented:change'])
  )
  expect(await page.evaluate(() => (window as unknown as { violations: string[] }).violations)).toEqual([])
})

test('the kit follows data-theme: the same elements read dark on a dark document', async ({ page }) => {
  await page.goto(`${origin}/frame`)
  const bg = (): Promise<string> =>
    page.evaluate(() => getComputedStyle(document.querySelector('ui-button')!.shadowRoot!.querySelector('button')!).backgroundColor)
  const light = await bg()
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'dark'
    document.documentElement.style.colorScheme = 'dark'
  })
  const dark = await bg()
  expect(light).not.toBe(dark)
  const text = await page.evaluate(() => getComputedStyle(document.body).color)
  expect(text).toBe('rgb(255, 255, 255)')
})

test('standalone, with no host to set data-theme, the kit follows the system setting; an explicit data-theme wins', async ({ page }) => {
  const read = (): Promise<{ body: string; button: string }> =>
    page.evaluate(() => ({
      body: getComputedStyle(document.body).color,
      button: getComputedStyle(document.querySelector('ui-segmented')!.shadowRoot!.querySelector('.group')!).backgroundColor
    }))
  await page.emulateMedia({ colorScheme: 'light' })
  await page.goto(`${origin}/standalone`)
  expect(await page.evaluate(() => document.documentElement.hasAttribute('data-theme'))).toBe(false)
  const light = await read()
  expect(light.body).toBe('rgb(0, 0, 0)')

  await page.emulateMedia({ colorScheme: 'dark' })
  const dark = await read()
  expect(dark.body).toBe('rgb(255, 255, 255)')
  expect(dark.button).not.toBe(light.button)

  // the host (when there is one) says light on a dark system: that wins
  await page.evaluate(() => (document.documentElement.dataset.theme = 'light'))
  expect((await read()).body).toBe('rgb(0, 0, 0)')
  // and dark on a light system
  await page.emulateMedia({ colorScheme: 'light' })
  await page.evaluate(() => (document.documentElement.dataset.theme = 'dark'))
  expect((await read()).body).toBe('rgb(255, 255, 255)')
})
