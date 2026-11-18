import { mkdir, writeFile } from 'node:fs/promises'
import { join, normalize, resolve, sep } from 'node:path'
import { pathToFileURL } from 'node:url'
import type { ServerWebSocket } from 'bun'
import { createRpcPeer, RpcError, RpcErrorCode, type RpcPeer } from '@agentos/sdk/rpc'
import { createProtocolDispatcher, type HostEmitter } from '@agentos/sdk/host/server'
import type { AppManifest } from '@agentos/sdk'
import type { RegistryApp } from '@agentos/sdk/host/protocol'
import { AgentHost, type AgentDefinition } from './acp/agentHost'
import { agentDefinitions } from './acp/agents'
import { AppRuntime } from './apps/runtime'
import type { ServerConfig } from './config'
import { SystemDb } from './db'
import { createMcpHandler } from './mcp/server'
import { Registries } from './registry/registries'
import { ToolRegistry } from './tools/registry'

const VERSION = '0.1.0'

export interface RunningServer {
  url: URL
  runtime: AppRuntime
  /**
   * Dev: re-install apps whose bundles changed in the local registry (after
   * `bun run build:apps`), exercising the same unregister → register path as
   * an update. Returns the ids reloaded.
   */
  syncLocal(): Promise<string[]>
  stop(): Promise<void>
}

interface SocketData {
  peer?: RpcPeer
}

/** Resolve `rel` inside `root`, or undefined if it would escape it. */
function within(root: string, rel: string): string | undefined {
  const target = normalize(join(root, decodeURIComponent(rel)))
  return target === root || target.startsWith(root + sep) ? target : undefined
}

async function serveFile(path: string | undefined, headers: Record<string, string> = {}): Promise<Response | undefined> {
  if (!path) return undefined
  const file = Bun.file(path)
  return (await file.exists()) ? new Response(file, { headers }) : undefined
}

export async function startServer(
  config: ServerConfig,
  options: { agents?: AgentDefinition[] } = {}
): Promise<RunningServer> {
  await mkdir(config.dataDir, { recursive: true })
  const db = new SystemDb(join(config.dataDir, 'agentos.sqlite'))
  const tools = new ToolRegistry()
  const sockets = new Set<ServerWebSocket<SocketData>>()

  const emit: HostEmitter = (channel, payload) => {
    for (const ws of sockets) ws.data.peer?.notify(channel, [payload])
  }

  const runtime = new AppRuntime(db, tools, emit, config.dataDir)
  const registries = new Registries([
    pathToFileURL(join(config.localRegistryDir, 'registry.json')),
    ...config.registries.map((u) => new URL(u))
  ])
  const mcp = createMcpHandler(tools, VERSION)

  // Bun.serve with port 0 picks a free port; the MCP URL needs the real one.
  let mcpUrl = ''
  const workspace = join(config.dataDir, 'workspace')
  await mkdir(workspace, { recursive: true })
  const agents = new AgentHost(
    options.agents ?? agentDefinitions(),
    {
      get url() {
        return mcpUrl
      },
      token: mcp.token
    },
    {
      update: (agentId, notification) => emit('agents:update', { agentId, notification }),
      permission: (request) => emit('agents:permission', request),
      exit: (agentId, code) => emit('agents:exit', { agentId, code })
    },
    workspace
  )

  const install = async (registry: string, appId: string): Promise<ReturnType<AppRuntime['list']>[number]> => {
    const { source, entry } = await registries.find(registry, appId)
    const [server, client] = await Promise.all([
      registries.download(source, entry.server),
      registries.download(source, entry.client)
    ])
    // The registry schema validated the manifest; its icon is a free string there.
    return runtime.install(source.registry.name, entry.manifest as AppManifest, {
      server,
      serverSha: entry.server.sha256,
      client,
      clientSha: entry.client.sha256
    })
  }

  const dispatcher = createProtocolDispatcher({
    invoke: {
      'host:info': () => ({ name: 'AgentOS', version: VERSION, platform: process.platform, versions: { bun: Bun.version } }),
      'apps:list': () => runtime.list(),
      'apps:install': (registry, appId) => install(registry, appId),
      'apps:uninstall': (appId) => runtime.uninstall(appId),
      'registry:list': async () => {
        const out: RegistryApp[] = []
        for (const { registry } of await registries.load()) {
          for (const entry of registry.apps) {
            out.push({
              registry: registry.name,
              manifest: entry.manifest as AppManifest,
              installedVersion: runtime.installedRow(entry.manifest.id)?.version ?? null
            })
          }
        }
        return out
      },
      'apps:invoke': (appId, channel, payload) => runtime.invoke(appId, channel, payload),
      'tools:list': () => tools.list(),
      'tools:call': (id, input) => tools.call(id, input),
      'agents:list': () => agents.list(),
      'agents:new-session': (agentId) => agents.newSession(agentId),
      'agents:prompt': (agentId, sessionId, text) => agents.prompt(agentId, sessionId, text),
      'agents:cancel': (agentId, sessionId) => agents.cancel(agentId, sessionId),
      'agents:set-mode': (agentId, sessionId, modeId) => agents.setMode(agentId, sessionId, modeId),
      'agents:set-config': (agentId, sessionId, configId, value) => agents.setConfig(agentId, sessionId, configId, value)
    },
    send: {
      'agents:permission-response': (id, optionId) => agents.respondPermission(id, optionId)
    }
  })

  await runtime.start()

  const clientRoot = resolve(config.clientDir)
  const registryRoot = resolve(config.localRegistryDir)

  const server = Bun.serve<SocketData>({
    hostname: config.hostname,
    port: config.port,
    async fetch(request, srv) {
      const url = new URL(request.url)

      if (url.pathname === '/rpc') {
        // Browsers always send Origin on WebSocket upgrades; refuse other sites' pages.
        const origin = request.headers.get('origin')
        if (origin && origin !== url.origin && !config.allowedOrigins.includes(origin)) {
          return new Response('Forbidden origin', { status: 403 })
        }
        return srv.upgrade(request, { data: {} }) ? undefined : new Response('Expected a WebSocket', { status: 426 })
      }

      if (url.pathname === '/mcp') return mcp.handle(request)

      if (url.pathname.startsWith('/bundles/')) {
        const [, , id, version, file] = url.pathname.split('/')
        if (file === 'client.js' && id && version) {
          const res = await serveFile(runtime.clientBundle(id, version), {
            'content-type': 'text/javascript; charset=utf-8',
            'cache-control': 'public, max-age=31536000, immutable'
          })
          if (res) return res
        }
        return new Response('Not found', { status: 404 })
      }

      // This repo is a registry too: other AgentOS servers can install from /registry/registry.json.
      if (url.pathname.startsWith('/registry/')) {
        const res = await serveFile(within(registryRoot, url.pathname.slice('/registry/'.length)))
        return res ?? new Response('Not found', { status: 404 })
      }

      const res = await serveFile(within(clientRoot, url.pathname === '/' ? 'index.html' : url.pathname.slice(1)))
      if (res) return res
      const shell = await serveFile(join(clientRoot, 'index.html'))
      return shell ?? new Response('The AgentOS client is not built. Run `bun run build`.', { status: 503 })
    },
    websocket: {
      open(ws) {
        sockets.add(ws)
        ws.data.peer = createRpcPeer({
          send: (m) => ws.send(m),
          ...dispatcher,
          toRpcError: (error) =>
            error instanceof RpcError
              ? { code: error.code, message: error.message, data: error.data }
              : { code: RpcErrorCode.InternalError, message: error instanceof Error ? error.message : String(error) },
          onError: (error) => console.warn('[agentos] rpc:', error)
        })
      },
      message(ws, message) {
        ws.data.peer?.receive(typeof message === 'string' ? message : new TextDecoder().decode(message))
      },
      close(ws) {
        sockets.delete(ws)
        ws.data.peer?.close()
      }
    }
  })

  const origin = new URL(`http://${server.hostname === '0.0.0.0' ? '127.0.0.1' : server.hostname}:${server.port}`)
  mcpUrl = new URL('/mcp', origin).href
  // Lets other local MCP clients (an agent in a terminal) use the same tools.
  await writeFile(
    join(config.dataDir, 'mcp.json'),
    JSON.stringify({ url: mcpUrl, headers: { Authorization: `Bearer ${mcp.token}` } }, null, 2),
    { mode: 0o600 }
  )
  console.log(`[agentos] server on ${origin.href} · MCP ${mcpUrl} · ${runtime.list().length} apps · data ${config.dataDir}`)

  const localRegistry = pathToFileURL(join(config.localRegistryDir, 'registry.json')).href

  return {
    url: origin,
    runtime,
    async syncLocal() {
      const local = (await registries.load()).find((r) => r.url.href === localRegistry)
      if (!local) return []
      const reloaded: string[] = []
      for (const entry of local.registry.apps) {
        const row = runtime.installedRow(entry.manifest.id)
        if (!row || row.registry !== local.registry.name) continue
        if (row.serverSha === entry.server.sha256 && row.clientSha === entry.client.sha256) continue
        await install(local.registry.name, entry.manifest.id)
        reloaded.push(entry.manifest.id)
      }
      return reloaded
    },
    async stop() {
      agents.stopAll()
      for (const ws of sockets) ws.close(1001, 'Server stopping')
      await server.stop(true)
      await runtime.stopAll()
      db.close()
    }
  }
}

