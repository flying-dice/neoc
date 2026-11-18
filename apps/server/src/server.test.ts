/**
 * End-to-end server test: a real Bun server, the Calculator built into a temp
 * bundles directory, a temp known-apps list, and a real WebSocket JSON-RPC
 * client (the SDK's own bridge). No network beyond 127.0.0.1: "hosted" bundles
 * are served by a tiny static Bun.serve in this file.
 *
 * Covers: known apps and install by id, integrity checks, broken list entries,
 * a hosted base over http://127.0.0.1 with a private-repo token (sent to the
 * base origin only), persistence across a restart, uninstall / reinstall, MCP,
 * and the origin / validation guards.
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { cp, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { buildAppBundle } from '@agentos/sdk/build'
import { createWsHostBridge, type HostBridge } from '@agentos/sdk/host/client'
import type { InstalledApp } from '@agentos/sdk/host/protocol'
import { loadConfig, type ServerConfig } from './config'
import { startServer, type RunningServer } from './server'

const repo = resolve(import.meta.dir, '../../..')
let tmp: string
/** The built bundles: <tmp>/bundles/calculator/{bundle.json,0.1.0/...}. */
let bundlesDir: string
let counter = 0

/** Await a rejection explicitly (Bun's in-process WebSocket delivers it synchronously). */
async function rejection(promise: Promise<unknown>): Promise<Error> {
  try {
    await promise
  } catch (error) {
    return error as Error
  }
  throw new Error('expected the call to reject')
}

const nextChange = (bridge: HostBridge): Promise<InstalledApp[]> =>
  new Promise((resolve) => {
    const off = bridge.apps.onChanged((apps) => {
      off()
      resolve(apps)
    })
  })

const connect = (server: RunningServer): HostBridge => createWsHostBridge(new URL('/rpc', server.url).href.replace(/^http/, 'ws'))

/** Write a known-apps list into its own temp directory and return its path. */
async function writeList(list: { base?: string; apps: string[] }): Promise<string> {
  const dir = join(tmp, `list-${counter++}`)
  await mkdir(dir, { recursive: true })
  const file = join(dir, 'known-apps.json')
  await writeFile(file, JSON.stringify(list))
  return file
}

function configFor(knownApps: string, env: Record<string, string> = {}): ServerConfig {
  return loadConfig({
    AGENTOS_PORT: '0',
    AGENTOS_DATA: join(tmp, `data-${counter++}`),
    AGENTOS_KNOWN_APPS: knownApps,
    AGENTOS_CLIENT_DIR: join(tmp, 'no-client'),
    ...env
  })
}

/** Run `body` against a fresh server, always stopping it. */
async function withServer(config: ServerConfig, body: (host: HostBridge, server: RunningServer) => Promise<void>): Promise<void> {
  const server = await startServer(config, { agents: [] })
  try {
    await body(connect(server), server)
  } finally {
    await server.stop()
  }
}

interface Seen {
  path: string
  authorization: string | null
}

/** A static file server over `dir` on 127.0.0.1 that records what it was asked for. `redirectTo` bounces /redir/* to another origin. */
function serveDir(dir: string, redirectTo?: string): { origin: string; seen: Seen[]; stop(): void } {
  const seen: Seen[] = []
  const server = Bun.serve({
    hostname: '127.0.0.1',
    port: 0,
    async fetch(request) {
      const { pathname } = new URL(request.url)
      seen.push({ path: pathname, authorization: request.headers.get('authorization') })
      if (redirectTo && pathname.startsWith('/redir/')) return Response.redirect(new URL(pathname.slice('/redir'.length), redirectTo).href, 302)
      const file = Bun.file(join(dir, decodeURIComponent(pathname)))
      return (await file.exists()) ? new Response(file) : new Response('Not found', { status: 404 })
    }
  })
  return { origin: `http://127.0.0.1:${server.port}`, seen, stop: () => void server.stop(true) }
}

beforeAll(async () => {
  tmp = await mkdtemp(join(tmpdir(), 'agentos-server-'))
  bundlesDir = join(tmp, 'bundles')
  await buildAppBundle({ appDir: join(repo, 'apps/server/test-fixtures/calculator'), outDir: bundlesDir })
})

afterAll(() => rm(tmp, { recursive: true, force: true }))

describe('AgentOS server', () => {
  let config: ServerConfig
  let server: RunningServer
  let host: HostBridge

  beforeAll(async () => {
    // No `base`: entries resolve against the list file itself.
    config = configFor(await writeList({ apps: [pathToFileURL(join(bundlesDir, 'calculator/bundle.json')).href] }))
    server = await startServer(config, { agents: [] })
    host = connect(server)
  })

  afterAll(() => server?.stop())

  test('host info and an empty install', async () => {
    const info = await host.host.info()
    expect(info.name).toBe('AgentOS')
    expect(info.versions.bun).toBe(Bun.version)
    expect(await host.apps.list()).toEqual([])
  })

  test('the known-apps list offers the Calculator, not installed yet', async () => {
    const known = await host.apps.known()
    expect(known).toEqual([expect.objectContaining({ installedVersion: null })])
    expect(known[0].manifest.id).toBe('calculator')
    expect(known[0].source).toEndWith('/calculator/bundle.json')
  })

  test('install by id registers the server module, its tools and handlers, and notifies clients', async () => {
    const changed = nextChange(host)
    const app = await host.apps.install('calculator')
    expect(app).toMatchObject({ status: 'running', manifest: { version: '0.1.0' } })
    expect((await changed).map((a) => a.manifest.id)).toEqual(['calculator'])
    expect((await host.apps.known())[0].installedVersion).toBe('0.1.0')

    expect((await host.tools.list()).map((t) => t.id).sort()).toEqual([
      'calculator__calculate',
      'calculator__history',
      'calculator__show_on_calculator'
    ])
    expect(await host.tools.call('calculator__calculate', { expression: '(12.5 + 7) * 3' })).toMatchObject({ result: 58.5 })
    expect(await host.apps.invoke('calculator', 'evaluate', '6 * 7')).toBe(42)
  })

  test('the client bundle is served at clientUrl and reads Svelte from the host', async () => {
    const [app] = await host.apps.list()
    const res = await fetch(new URL(app.clientUrl, server.url))
    expect(res.status).toBe(200)
    expect(res.headers.get('content-type')).toContain('javascript')
    const code = await res.text()
    expect(code).toContain('__AGENTOS_SHARED__')
    expect(code).not.toMatch(/from\s*["']svelte/)
  })

  test('tools are reachable over MCP with the bearer token, and not without it', async () => {
    const discovery = (await Bun.file(join(config.dataDir, 'mcp.json')).json()) as { url: string; headers: Record<string, string> }
    const call = (body: unknown, headers: Record<string, string>): Promise<Response> =>
      fetch(discovery.url, {
        method: 'POST',
        headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', ...headers },
        body: JSON.stringify(body)
      })
    expect((await call({ jsonrpc: '2.0', id: 1, method: 'tools/list' }, {})).status).toBe(401)
    const listed = (await (await call({ jsonrpc: '2.0', id: 1, method: 'tools/list' }, discovery.headers)).json()) as {
      result: { tools: { name: string }[] }
    }
    expect(listed.result.tools.map((t) => t.name)).toContain('calculator__calculate')
  })

  test('bad input is rejected with JSON-RPC errors, not crashes', async () => {
    expect((await rejection(host.apps.install('NOT VALID'))).message).toMatch(/Invalid params/)
    expect((await rejection(host.apps.install('nope'))).message).toMatch(/Unknown app "nope"/)
    expect((await rejection(host.tools.call('calculator__calculate', { expression: '1 / 0' }))).message).toMatch(/Division by zero/)
  })

  test('clients install only known apps: there is no way to name a URL', async () => {
    const url = pathToFileURL(join(bundlesDir, 'calculator/bundle.json')).href
    expect((await rejection(host.apps.install(url))).message).toMatch(/Invalid params/)
  })

  test('pages from other origins cannot open /rpc', async () => {
    const res = await fetch(new URL('/rpc', server.url), {
      headers: { upgrade: 'websocket', connection: 'Upgrade', origin: 'https://evil.example', 'sec-websocket-version': '13', 'sec-websocket-key': 'dGhlIHNhbXBsZSBub25jZQ==' }
    })
    expect(res.status).toBe(403)
  })

  test('there is no /registry route', async () => {
    expect((await fetch(new URL('/registry/registry.json', server.url))).status).not.toBe(200)
  })

  test('installs and storage survive a restart', async () => {
    await host.tools.call('calculator__calculate', { expression: '2 + 2' })
    await server.stop()
    server = await startServer(config, { agents: [] })
    host = connect(server)
    expect((await host.apps.list()).map((a) => a.status)).toEqual(['running'])
    const history = (await host.tools.call('calculator__history', { limit: 5 })) as { entries: { expression: string }[] }
    expect(history.entries.map((e) => e.expression)).toContain('2 + 2')
  })

  test('uninstall unregisters tools and handlers; reinstall brings them back', async () => {
    await host.apps.uninstall('calculator')
    expect(await host.apps.list()).toEqual([])
    expect(await host.tools.list()).toEqual([])
    expect((await host.apps.known())[0].installedVersion).toBeNull()
    expect((await rejection(host.apps.invoke('calculator', 'evaluate', '1+1'))).message).toMatch(/not running/)
    await host.apps.install('calculator')
    expect(await host.apps.invoke('calculator', 'evaluate', '1+1')).toBe(2)
  })
})

describe('known-apps list', () => {
  test('a broken entry is skipped while the others still load', async () => {
    const broken = join(tmp, 'broken-bundles')
    await cp(bundlesDir, broken, { recursive: true })
    await mkdir(join(broken, 'garbled'), { recursive: true })
    await writeFile(join(broken, 'garbled/bundle.json'), '{ not json')
    await mkdir(join(broken, 'wrong-shape'), { recursive: true })
    await writeFile(join(broken, 'wrong-shape/bundle.json'), JSON.stringify({ manifest: { id: 'x' } }))
    const list = await writeList({
      base: pathToFileURL(broken).href + '/',
      apps: ['missing/bundle.json', 'garbled/bundle.json', 'wrong-shape/bundle.json', 'calculator/bundle.json', 'http://[bad']
    })
    await withServer(configFor(list), async (host) => {
      expect((await host.apps.known()).map((a) => a.manifest.id)).toEqual(['calculator'])
      expect(await host.apps.install('calculator')).toMatchObject({ status: 'running' })
    })
  })

  test('a tampered bundle fails the integrity check and installs nothing', async () => {
    const tampered = join(tmp, 'tampered-bundles')
    await cp(bundlesDir, tampered, { recursive: true })
    const serverJs = join(tampered, 'calculator/0.1.0/server.js')
    await writeFile(serverJs, (await Bun.file(serverJs).text()) + '\n// injected')
    const list = await writeList({ base: pathToFileURL(tampered).href + '/', apps: ['calculator/bundle.json'] })
    await withServer(configFor(list), async (host) => {
      expect((await rejection(host.apps.install('calculator'))).message).toMatch(/Integrity check failed/)
      expect(await host.apps.list()).toEqual([])
      expect(await host.tools.list()).toEqual([])
    })
  })

  test('AGENTOS_APPS_BASE (a path) overrides the list base', async () => {
    const list = await writeList({ base: 'https://unreachable.invalid/bundles/', apps: ['calculator/bundle.json'] })
    await withServer(configFor(list, { AGENTOS_APPS_BASE: bundlesDir }), async (host, running) => {
      expect((await host.apps.known()).map((a) => a.manifest.id)).toEqual(['calculator'])
      expect((await running.known.base())?.href).toBe(pathToFileURL(bundlesDir).href + '/')
    })
  })

  test('AGENTOS_APPS adds extra absolute bundle.json URLs', async () => {
    const list = await writeList({ apps: [] })
    const extra = pathToFileURL(join(bundlesDir, 'calculator/bundle.json')).href
    await withServer(configFor(list, { AGENTOS_APPS: extra }), async (host) => {
      expect((await host.apps.known()).map((a) => a.manifest.id)).toEqual(['calculator'])
    })
  })

  test('dev reload: a rebuilt bundle re-installs the running app', async () => {
    const dev = join(tmp, 'dev-bundles')
    await cp(bundlesDir, dev, { recursive: true })
    const list = await writeList({ base: pathToFileURL(dev).href + '/', apps: ['calculator/bundle.json'] })
    await withServer(configFor(list), async (host, running) => {
      await host.apps.install('calculator')
      expect(await running.reloadChanged()).toEqual([])

      // Publish "0.1.1" as the latest, with a changed client.js, as a rebuilt dev bundle would.
      const next = join(dev, 'calculator/0.1.1')
      await cp(join(dev, 'calculator/0.1.0'), next, { recursive: true })
      const clientJs = (await Bun.file(join(next, 'client.js')).text()) + '\n// 0.1.1'
      await writeFile(join(next, 'client.js'), clientJs)
      const clientSha = new Bun.CryptoHasher('sha256').update(clientJs).digest('hex')
      const bump = async (file: string, prefix: string): Promise<void> => {
        const d = (await Bun.file(file).json()) as { manifest: { version: string }; server: { url: string }; client: { url: string; sha256: string } }
        d.manifest.version = '0.1.1'
        d.client.sha256 = clientSha
        d.server.url = `${prefix}server.js`
        d.client.url = `${prefix}client.js`
        await writeFile(file, JSON.stringify(d))
      }
      await bump(join(next, 'bundle.json'), '')
      await bump(join(dev, 'calculator/bundle.json'), '0.1.1/')

      expect(await running.reloadChanged()).toEqual(['calculator'])
      expect((await host.apps.list())[0]).toMatchObject({ manifest: { version: '0.1.1' }, status: 'running' })
      expect(await running.reloadChanged()).toEqual([])
    })
  })
})

describe('a hosted base over http://127.0.0.1', () => {
  test('installs by id from static files', async () => {
    const hosted = serveDir(bundlesDir)
    try {
      const list = await writeList({ base: `${hosted.origin}/`, apps: ['calculator/bundle.json'] })
      await withServer(configFor(list), async (host) => {
        expect((await host.apps.known())[0]).toMatchObject({ installedVersion: null, source: `${hosted.origin}/calculator/bundle.json` })
        expect(await host.apps.install('calculator')).toMatchObject({ status: 'running' })
        expect(await host.apps.invoke('calculator', 'evaluate', '3 * 3')).toBe(9)
      })
      expect(hosted.seen.map((s) => s.path)).toEqual(expect.arrayContaining(['/calculator/bundle.json', '/calculator/0.1.0/server.js', '/calculator/0.1.0/client.js']))
      expect(hosted.seen.every((s) => s.authorization === null)).toBe(true)
    } finally {
      hosted.stop()
    }
  })

  test('AGENTOS_APPS_BASE points the same list at another host', async () => {
    const hosted = serveDir(bundlesDir)
    try {
      const list = await writeList({ base: 'https://unreachable.invalid/bundles/', apps: ['calculator/bundle.json'] })
      await withServer(configFor(list, { AGENTOS_APPS_BASE: `${hosted.origin}/` }), async (host) => {
        expect(await host.apps.install('calculator')).toMatchObject({ status: 'running' })
      })
    } finally {
      hosted.stop()
    }
  })

  test('the token goes to the base origin only: not to other hosts, not across a redirect', async () => {
    const other = serveDir(bundlesDir)
    const base = serveDir(bundlesDir, other.origin)
    try {
      const list = await writeList({
        base: `${base.origin}/`,
        // Direct from the base; an absolute URL on another host; a path the base redirects to the other host.
        apps: ['calculator/bundle.json', `${other.origin}/calculator/bundle.json`, 'redir/calculator/bundle.json']
      })
      await withServer(configFor(list, { AGENTOS_APPS_TOKEN: 'ghp_secret' }), async (host) => {
        expect((await host.apps.known()).map((a) => a.manifest.id)).toEqual(['calculator'])
        await host.apps.install('calculator')
      })
      expect(base.seen.length).toBeGreaterThan(0)
      expect(base.seen.every((s) => s.authorization === 'Bearer ghp_secret')).toBe(true)
      expect(other.seen.length).toBeGreaterThan(0)
      expect(other.seen.every((s) => s.authorization === null)).toBe(true)
    } finally {
      base.stop()
      other.stop()
    }
  })

  test('a hosted descriptor cannot point the server at a local file', async () => {
    const evil = join(tmp, 'evil-bundles')
    await cp(bundlesDir, evil, { recursive: true })
    const descriptor = (await Bun.file(join(evil, 'calculator/bundle.json')).json()) as { server: { url: string } }
    descriptor.server.url = pathToFileURL(join(bundlesDir, 'calculator/0.1.0/server.js')).href
    await writeFile(join(evil, 'calculator/bundle.json'), JSON.stringify(descriptor))
    const hosted = serveDir(evil)
    try {
      const list = await writeList({ base: `${hosted.origin}/`, apps: ['calculator/bundle.json'] })
      await withServer(configFor(list), async (host) => {
        expect((await rejection(host.apps.install('calculator'))).message).toMatch(/points at a local file/)
        expect(await host.apps.list()).toEqual([])
      })
    } finally {
      hosted.stop()
    }
  })

  test('plain http to a non-local host is refused', async () => {
    const list = await writeList({ apps: ['http://example.com/calculator/bundle.json'] })
    await withServer(configFor(list), async (host) => {
      expect(await host.apps.known()).toEqual([])
    })
  })
})

describe('config', () => {
  test('defaults: port 7400 (7000 is macOS AirPlay), the built-in list, no override or token', () => {
    const config = loadConfig({})
    expect(config.port).toBe(7400)
    expect(config.knownAppsFile).toEndWith('apps/server/known-apps.json')
    expect(config.appsBase).toBeUndefined()
    expect(config.appsToken).toBeUndefined()
  })

  test('AGENTOS_APPS_BASE accepts a URL or a path, always as a directory', () => {
    expect(loadConfig({ AGENTOS_APPS_BASE: 'https://cdn.example/agentos' }).appsBase?.href).toBe('https://cdn.example/agentos/')
    expect(loadConfig({ AGENTOS_APPS_BASE: 'file:///tmp/bundles/' }).appsBase?.href).toBe('file:///tmp/bundles/')
    expect(loadConfig({ AGENTOS_APPS_BASE: '/tmp/bundles' }).appsBase?.href).toBe('file:///tmp/bundles/')
    expect(loadConfig({ AGENTOS_APPS_BASE: '../../dist/bundles' }).appsBase?.href).toStartWith('file:///')
  })
})
