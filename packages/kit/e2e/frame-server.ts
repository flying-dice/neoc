/**
 * A stand-in for the host for the kit's own specs: it serves the built kit (`client/public/kit`) and a page that loads it
 * the way an app's document does, with the frame's content security policy. Specs of the kit use it so they check the kit,
 * not the host's wiring (the app journeys check that).
 */
import { readFileSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { join } from 'node:path'

const KIT = join(import.meta.dirname, '../../../client/public/kit')
/** The frame's policy, as the host sends it (NEOC-300; `allow-forms` so a native form works in an app, PR #56). */
export const CSP =
  "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; sandbox allow-scripts allow-forms"

/** The same without the sandbox: a standalone app is an ordinary full-screen page. */
const CSP_STANDALONE = CSP.replace('; sandbox allow-scripts allow-forms', '')

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
/** A document no vos page set up (Storybook, a developer's own test page): nothing sets data-theme. */
const STANDALONE = `<!doctype html>\n<html>\n${BODY}`

const PAGE_JS = `
document.querySelector('ui-segmented').options = [{ value: 'a', label: 'A' }, { value: 'b', label: 'B' }]
window.violations = []
document.addEventListener('securitypolicyviolation', (e) => window.violations.push(e.violatedDirective + ' ' + e.blockedURI))
`

export interface FrameServer {
  origin: string
  close(): Promise<void>
}

/** Starts the server on a free loopback port. Pages: `/frame` (embedded), `/standalone` (no data-theme, no sandbox). */
export async function startFrameServer(): Promise<FrameServer> {
  const server: Server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://x')
    if (url.pathname === '/frame' || url.pathname === '/standalone') {
      const framed = url.pathname === '/frame'
      res
        .writeHead(200, {
          'content-type': 'text/html',
          'content-security-policy': framed ? CSP : CSP_STANDALONE
        })
        .end(framed ? PAGE : STANDALONE)
    } else if (url.pathname === '/page.js') {
      res.writeHead(200, { 'content-type': 'text/javascript' }).end(PAGE_JS)
    } else if (url.pathname === '/kit/kit.js' || url.pathname === '/kit/tokens.css') {
      const type = url.pathname.endsWith('.js') ? 'text/javascript' : 'text/css'
      res
        .writeHead(200, { 'content-type': type })
        .end(readFileSync(join(KIT, url.pathname.slice(5))))
    } else res.writeHead(404).end()
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
  return { origin, close: () => new Promise((done) => server.close(() => done())) }
}
