import { app } from 'electron'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { HostEmitter, InvokeHandlers, SendHandlers } from '@agentos/sdk/host/main'
import { AppHost } from './appHost'
import { installedApps } from './installedApps'
import { startMcpServer, type McpEndpoint } from './mcpServer'
import { ToolRegistry } from './toolRegistry'
import { AgentHost } from './acp/agentHost'
import { agentDefinitions } from './acp/agents'

type PlatformInvoke = Pick<
  InvokeHandlers,
  | 'apps:invoke'
  | 'tools:list'
  | 'tools:call'
  | 'agents:list'
  | 'agents:new-session'
  | 'agents:prompt'
  | 'agents:cancel'
  | 'agents:set-mode'
  | 'agents:set-config'
>
type PlatformSend = Pick<SendHandlers, 'agents:permission-response'>

export interface Platform {
  tools: ToolRegistry
  apps: AppHost
  mcp: McpEndpoint
  agents: AgentHost
  /** This platform's share of the host IPC protocol. */
  handlers: { invoke: PlatformInvoke; send: PlatformSend }
  shutdown(): Promise<void>
}

/**
 * Boots the AgentOS platform in the main process:
 *   app modules → tool registry → MCP server → ACP agent host
 */
export async function startPlatform(emit: HostEmitter): Promise<Platform> {
  const tools = new ToolRegistry()
  const apps = new AppHost(tools, emit)
  await apps.load(installedApps)

  const mcp = await startMcpServer(tools, app.getVersion())
  writeDiscoveryFile(mcp)
  console.log(`[agentos] MCP server ${mcp.url} — ${tools.list().length} tools`)

  const workspace = join(app.getPath('userData'), 'workspace')
  mkdirSync(workspace, { recursive: true })

  const agents = new AgentHost(
    agentDefinitions(),
    mcp,
    {
      update: (agentId, notification) => emit('agents:update', { agentId, notification }),
      permission: (request) => emit('agents:permission', request),
      exit: (agentId, code) => emit('agents:exit', { agentId, code })
    },
    workspace
  )

  return {
    tools,
    apps,
    mcp,
    agents,
    handlers: {
      invoke: {
        'apps:invoke': (appId, channel, payload) => apps.invoke(appId, channel, payload),
        'tools:list': () => tools.list(),
        'tools:call': (id, input) => tools.call(id, input),
        'agents:list': () => agents.list(),
        'agents:new-session': (agentId) => agents.newSession(agentId),
        'agents:prompt': (agentId, sessionId, text) => agents.prompt(agentId, sessionId, text),
        'agents:cancel': (agentId, sessionId) => agents.cancel(agentId, sessionId),
        'agents:set-mode': (agentId, sessionId, modeId) =>
          agents.setMode(agentId, sessionId, modeId),
        'agents:set-config': (agentId, sessionId, configId, value) =>
          agents.setConfig(agentId, sessionId, configId, value)
      },
      send: {
        'agents:permission-response': (id, optionId) => agents.respondPermission(id, optionId)
      }
    },
    shutdown: async () => {
      agents.stopAll()
      await mcp.close()
    }
  }
}

/**
 * userData/mcp.json (owner-only) lets other local MCP clients — e.g. an agent
 * you run in a terminal — connect to the same AgentOS tools.
 */
function writeDiscoveryFile(mcp: McpEndpoint): void {
  writeFileSync(
    join(app.getPath('userData'), 'mcp.json'),
    JSON.stringify({ url: mcp.url, headers: { Authorization: `Bearer ${mcp.token}` } }, null, 2),
    { mode: 0o600 }
  )
}
