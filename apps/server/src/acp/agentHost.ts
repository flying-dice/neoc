import { spawn, type ChildProcess } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { Readable, Writable } from 'node:stream'
import * as acp from '@agentclientprotocol/sdk'
import type {
  AgentPermissionRequest,
  AgentSessionInfo,
  AgentSummary,
  ConfigValue
} from '@agentos/sdk/host/protocol'
import type { McpEndpoint } from '../mcp/server'

/** An installable ACP agent: any program that speaks ACP over stdio. */
export interface AgentDefinition {
  id: string
  name: string
  command: string
  args: string[]
  env?: Record<string, string>
}

export interface AgentHostEvents {
  /** Streaming session updates: message chunks, tool calls, plans… */
  update: (agentId: string, notification: acp.SessionNotification) => void
  /** The agent asks to run something; resolve with the user's choice. */
  permission: (request: AgentPermissionRequest) => void
  exit: (agentId: string, code: number | null) => void
}

interface RunningAgent {
  definition: AgentDefinition
  child: ChildProcess
  connection: acp.ClientConnection
  init: acp.InitializeResponse
}

const PERMISSION_TIMEOUT_MS = 2 * 60_000

/**
 * ACP client host. Spawns agents as child processes, speaks ACP over their
 * stdio, and gives every session the AgentOS MCP server so agents can use the
 * tools installed apps expose. Permission requests are forwarded to the UI.
 */
export class AgentHost {
  #running = new Map<string, RunningAgent>()
  #pendingPermissions = new Map<string, (outcome: acp.RequestPermissionOutcome) => void>()

  constructor(
    private readonly definitions: AgentDefinition[],
    private readonly mcp: McpEndpoint,
    private readonly events: AgentHostEvents,
    private readonly workspace: string
  ) {}

  list(): AgentSummary[] {
    return this.definitions.map((d) => ({
      id: d.id,
      name: d.name,
      running: this.#running.has(d.id)
    }))
  }

  /** Start (if needed) the agent and open a new session wired to AgentOS tools. */
  async newSession(agentId: string): Promise<AgentSessionInfo> {
    const agent = await this.#ensureRunning(agentId)
    const http = agent.init.agentCapabilities?.mcpCapabilities?.http ?? false
    if (!http) {
      console.warn(
        `[agentos] agent "${agentId}" does not support HTTP MCP; starting without AgentOS tools`
      )
    }
    const session = await agent.connection.agent.request(acp.methods.agent.session.new, {
      cwd: this.workspace,
      mcpServers: http
        ? [
            {
              type: 'http',
              name: 'agentos',
              url: this.mcp.url,
              headers: [{ name: 'Authorization', value: `Bearer ${this.mcp.token}` }]
            }
          ]
        : []
    })
    return {
      sessionId: session.sessionId,
      agentId,
      toolsConnected: http,
      modes: session.modes ?? null,
      configOptions: session.configOptions ?? []
    }
  }

  /** Switch the session's mode (e.g. plan / accept edits). */
  async setMode(agentId: string, sessionId: string, modeId: string): Promise<void> {
    await this.#require(agentId).connection.agent.request(acp.methods.agent.session.setMode, {
      sessionId,
      modeId
    })
  }

  /** Change a session setting (model, reasoning effort, …); returns every option's new state. */
  async setConfig(
    agentId: string,
    sessionId: string,
    configId: string,
    value: ConfigValue
  ): Promise<acp.SessionConfigOption[]> {
    const response = await this.#require(agentId).connection.agent.request(
      acp.methods.agent.session.setConfigOption,
      typeof value === 'boolean'
        ? { sessionId, configId, type: 'boolean', value }
        : { sessionId, configId, value }
    )
    return response.configOptions
  }

  async prompt(agentId: string, sessionId: string, text: string): Promise<acp.StopReason> {
    const agent = this.#require(agentId)
    const response = await agent.connection.agent.request(acp.methods.agent.session.prompt, {
      sessionId,
      prompt: [{ type: 'text', text }]
    })
    return response.stopReason
  }

  async cancel(agentId: string, sessionId: string): Promise<void> {
    await this.#require(agentId).connection.agent.notify(acp.methods.agent.session.cancel, {
      sessionId
    })
  }

  respondPermission(id: string, optionId: string | null): void {
    const resolve = this.#pendingPermissions.get(id)
    if (!resolve) return
    this.#pendingPermissions.delete(id)
    resolve(optionId ? { outcome: 'selected', optionId } : { outcome: 'cancelled' })
  }

  stop(agentId: string): void {
    const agent = this.#running.get(agentId)
    if (!agent) return
    agent.connection.close()
    agent.child.kill()
    this.#running.delete(agentId)
  }

  stopAll(): void {
    for (const id of this.#running.keys()) this.stop(id)
  }

  // ── Internals ──────────────────────────────────────────────────────────

  #require(agentId: string): RunningAgent {
    const agent = this.#running.get(agentId)
    if (!agent) throw new Error(`Agent "${agentId}" is not running`)
    return agent
  }

  async #ensureRunning(agentId: string): Promise<RunningAgent> {
    const existing = this.#running.get(agentId)
    if (existing) return existing

    const definition = this.definitions.find((d) => d.id === agentId)
    if (!definition) throw new Error(`Unknown agent "${agentId}"`)

    const child = spawn(definition.command, definition.args, {
      cwd: this.workspace,
      env: { ...process.env, ...definition.env },
      stdio: ['pipe', 'pipe', 'inherit']
    })
    child.on('exit', (code) => {
      this.#running.delete(agentId)
      this.events.exit(agentId, code)
    })

    const stream = acp.ndJsonStream(
      Writable.toWeb(child.stdin!) as WritableStream<Uint8Array>,
      Readable.toWeb(child.stdout!) as ReadableStream<Uint8Array>
    )

    const client = acp
      .client({ name: 'AgentOS' })
      .onNotification(acp.methods.client.session.update, ({ params }) => {
        this.events.update(agentId, params)
      })
      .onRequest(acp.methods.client.session.requestPermission, ({ params, signal }) =>
        this.#askPermission(agentId, params, signal).then((outcome) => ({ outcome }))
      )

    const connection = client.connect(stream)
    try {
      const init = await connection.agent.request(acp.methods.agent.initialize, {
        protocolVersion: acp.PROTOCOL_VERSION,
        // AgentOS doesn't proxy the filesystem or terminals (yet) — agents use their own.
        clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
        clientInfo: { name: 'agentos', title: 'AgentOS', version: '0.1.0' }
      })
      const running: RunningAgent = { definition, child, connection, init }
      this.#running.set(agentId, running)
      return running
    } catch (error) {
      connection.close()
      child.kill()
      throw error
    }
  }

  #askPermission(
    agentId: string,
    params: acp.RequestPermissionRequest,
    signal: AbortSignal
  ): Promise<acp.RequestPermissionOutcome> {
    const id = randomUUID()
    return new Promise((resolve) => {
      const finish = (outcome: acp.RequestPermissionOutcome): void => {
        clearTimeout(timer)
        this.#pendingPermissions.delete(id)
        resolve(outcome)
      }
      const timer = setTimeout(() => finish({ outcome: 'cancelled' }), PERMISSION_TIMEOUT_MS)
      signal.addEventListener('abort', () => finish({ outcome: 'cancelled' }), { once: true })
      this.#pendingPermissions.set(id, finish)
      this.events.permission({ id, agentId, params })
    })
  }
}
