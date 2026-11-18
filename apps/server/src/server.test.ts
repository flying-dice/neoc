/**
 * End-to-end server test: a real Bun server, a real registry built from this
 * repo's Calculator, and a real WebSocket JSON-RPC client (the SDK's own
 * bridge). Covers install → use → uninstall → reinstall, persistence across a
 * restart, MCP, and the origin / validation guards.
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { buildRegistry } from '@agentos/sdk/build'
import { createWsHostBridge, type HostBridge } from '@agentos/sdk/host/client'
import type { InstalledApp } from '@agentos/sdk/host/protocol'
import { loadConfig, type ServerConfig } from './config'
import { startServer, type RunningServer } from './server'

const repo = resolve(import.meta.dir, '../../..')
let tmp: string
let config: ServerConfig
let server: RunningServer
let host: HostBridge

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

beforeAll(async () => {
  tmp = await mkdtemp(join(tmpdir(), 'agentos-server-'))
  const registryDir = join(tmp, 'registry')
  await buildRegistry({ name: 'agentos', appDirs: [join(repo, 'apps/calculator')], outDir: registryDir })
  config = loadConfig({ AGENTOS_PORT: '0', AGENTOS_DATA: join(tmp, 'data'), AGENTOS_REGISTRY_DIR: registryDir, AGENTOS_CLIENT_DIR: join(tmp, 'no-client') })
  server = await startServer(config, { agents: [] })
  host = createWsHostBridge(new URL('/rpc', server.url).href.replace(/^http/, 'ws'))
})

afterAll(async () => {
  await server?.stop()
  await rm(tmp, { recursive: true, force: true })
})

describe('AgentOS server', () => {
  test('host info and an empty install', async () => {
    const info = await host.host.info()
    expect(info.name).toBe('AgentOS')
    expect(info.versions.bun).toBe(Bun.version)
    expect(await host.apps.list()).toEqual([])
  })

  test('the local registry offers the Calculator', async () => {
    const available = await host.apps.available()
    expect(available).toEqual([expect.objectContaining({ registry: 'agentos', installedVersion: null })])
    expect(available[0].manifest.id).toBe('calculator')
  })

  test('install registers the server module, its tools and handlers, and notifies clients', async () => {
    const changed = nextChange(host)
    const app = await host.apps.install('agentos', 'calculator')
    expect(app).toMatchObject({ status: 'running', registry: 'agentos' })
    expect((await changed).map((a) => a.manifest.id)).toEqual(['calculator'])

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
    expect((await rejection(host.apps.install('agentos', 'NOT VALID'))).message).toMatch(/Invalid params/)
    expect((await rejection(host.apps.install('nope', 'calculator'))).message).toMatch(/Unknown registry/)
    expect((await rejection(host.tools.call('calculator__calculate', { expression: '1 / 0' }))).message).toMatch(/Division by zero/)
  })

  test('pages from other origins cannot open /rpc', async () => {
    const res = await fetch(new URL('/rpc', server.url), {
      headers: { upgrade: 'websocket', connection: 'Upgrade', origin: 'https://evil.example', 'sec-websocket-version': '13', 'sec-websocket-key': 'dGhlIHNhbXBsZSBub25jZQ==' }
    })
    expect(res.status).toBe(403)
  })

  test('the repo registry is served at /registry', async () => {
    const res = await fetch(new URL('/registry/registry.json', server.url))
    expect(((await res.json()) as { name: string }).name).toBe('agentos')
  })

  test('installs and storage survive a restart', async () => {
    await host.tools.call('calculator__calculate', { expression: '2 + 2' })
    await server.stop()
    server = await startServer(config, { agents: [] })
    host = createWsHostBridge(new URL('/rpc', server.url).href.replace(/^http/, 'ws'))
    expect((await host.apps.list()).map((a) => a.status)).toEqual(['running'])
    const history = (await host.tools.call('calculator__history', { limit: 5 })) as { entries: { expression: string }[] }
    expect(history.entries.map((e) => e.expression)).toContain('2 + 2')
  })

  test('uninstall unregisters tools and handlers; reinstall brings them back', async () => {
    await host.apps.uninstall('calculator')
    expect(await host.apps.list()).toEqual([])
    expect(await host.tools.list()).toEqual([])
    expect((await rejection(host.apps.invoke('calculator', 'evaluate', '1+1'))).message).toMatch(/not running/)
    await host.apps.install('agentos', 'calculator')
    expect(await host.apps.invoke('calculator', 'evaluate', '1+1')).toBe(2)
  })
})
