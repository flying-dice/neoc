/**
 * @agentos/sdk/testing — run the AgentOS renderer with no Electron and no
 * main process.
 *
 *   const host = createMemoryHost({ apps: [{ manifest, main }], agents: [demoAgent()] })
 *   Object.defineProperty(window, 'agentos', { value: host })   // whole desktop
 *   provideAppApi(appApiFor(host, 'calculator'))                 // one app (stories)
 *
 * The memory host implements the full `HostBridge`: it runs app main modules
 * in memory (handlers, tools with zod validation, storage, events), and plays
 * scripted agents that stream ACP-shaped session updates.
 */
import { z } from 'zod'
import type { AppManifest } from '../manifest'
import type { AppDispose, AppServerContext, AppServerModule } from '../app/server'
import type { AppClientApi } from '../app/client'
import type { HostBridge, Unsubscribe } from '../host/bridge'
import type {
  AgentExit,
  AgentPermissionRequest,
  AgentSessionInfo,
  AgentSummary,
  AgentUpdate,
  AppEvent,
  InstalledApp,
  SessionConfigOption,
  SessionModeState
} from '../host/protocol'
import { createAppRuntime } from '../host/client'
import { qualifyTool, type AppToolDefinition, type ToolInfo, type ToolResult } from '../tools'

// ── Agents ───────────────────────────────────────────────────────────────

/** What a scripted agent can do while answering one prompt. */
export interface AgentTurn {
  readonly sessionId: string
  /** Stream agent text (chunks are concatenated in the transcript). */
  say(text: string): Promise<void>
  think(text: string): Promise<void>
  /**
   * Run a tool from the registry as the agent would through MCP, showing a
   * tool-call row while it runs. Returns the tool's result.
   */
  useTool(id: string, args?: Record<string, unknown>, title?: string): Promise<ToolResult>
  /** Ask the user for permission; resolves with the chosen option id, or null when denied. */
  ask(title: string, options?: { optionId: string; name: string; kind: string }[]): Promise<string | null>
  /** True once the user pressed Stop. */
  readonly cancelled: boolean
}

export interface ScriptedAgent {
  id: string
  name: string
  /** Answer one prompt. Return an ACP stop reason (default "end_turn"). */
  respond(text: string, turn: AgentTurn): Promise<string | void>
  /** Whether the agent accepts the AgentOS MCP server (default true). */
  toolsConnected?: boolean
  /** Session modes offered (default: a deterministic Default / Plan pair). */
  modes?: SessionModeState | null
  /** Session config options offered (default: a deterministic model + effort pair). */
  configOptions?: SessionConfigOption[]
}

/** Deterministic defaults so UI tests can exercise model / effort / mode controls. */
export const defaultScriptedModes = (): SessionModeState => ({
  currentModeId: 'default',
  availableModes: [
    { id: 'default', name: 'Default', description: 'Ask before acting' },
    { id: 'plan', name: 'Plan', description: 'Plan without making changes' }
  ]
})

export const defaultScriptedConfig = (): SessionConfigOption[] => [
  {
    type: 'select',
    id: 'model',
    name: 'Model',
    category: 'model',
    currentValue: 'large',
    options: [
      { value: 'large', name: 'Large' },
      { value: 'small', name: 'Small' }
    ]
  },
  {
    type: 'select',
    id: 'effort',
    name: 'Effort',
    category: 'thought_level',
    currentValue: 'medium',
    options: [
      { value: 'low', name: 'Low' },
      { value: 'medium', name: 'Medium' },
      { value: 'high', name: 'High' }
    ]
  }
]

export interface MemoryApp {
  manifest: AppManifest
  /** The app's server module, run in the page (it gets no `dataDir`). */
  server?: AppServerModule
  /** Initial storage for the app. */
  storage?: Record<string, unknown>
}

export interface MemoryHostOptions {
  /** The host's known apps (what Discover offers). */
  apps?: MemoryApp[]
  /** Ids installed at start (default: every app in `apps`). */
  installed?: string[]
  agents?: ScriptedAgent[]
  platform?: string
  versions?: Record<string, string>
  /** Delay between streamed chunks, in ms (0 = next macrotask). */
  streamDelay?: number
}

export interface MemoryHost extends HostBridge {
  /** Resolves once every installed app's server module has finished `setup`. */
  readonly ready: Promise<void>
  /** Push an event to an app's client as if its server module emitted it. */
  emit(appId: string, event: string, payload?: unknown): void
  /** An app's in-memory storage (for assertions). */
  storageOf(appId: string): Map<string, unknown>
  /** Every call made through the bridge, for assertions. */
  readonly calls: { method: string; args: unknown[] }[]
}

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms))

function emitter<T>(): { on(l: (v: T) => void): Unsubscribe; fire(v: T): void } {
  const listeners = new Set<(v: T) => void>()
  return {
    on: (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    fire: (v) => listeners.forEach((l) => l(v))
  }
}

export function createMemoryHost(options: MemoryHostOptions = {}): MemoryHost {
  const delay = options.streamDelay ?? 0
  const calls: MemoryHost['calls'] = []
  const record = (method: string, args: unknown[]): void => void calls.push({ method, args })

  const appEvents = emitter<AppEvent>()
  const openRequests = emitter<string>()
  const updates = emitter<AgentUpdate>()
  const permissions = emitter<AgentPermissionRequest>()
  const exits = emitter<AgentExit>()

  const handlers = new Map<string, Map<string, (payload: unknown) => unknown>>()
  const storages = new Map<string, Map<string, unknown>>()
  const tools = new Map<string, { info: ToolInfo; def: AppToolDefinition }>()

  const changes = emitter<InstalledApp[]>()
  const catalog = new Map((options.apps ?? []).map((a) => [a.manifest.id, a]))
  const installed = new Map<string, { controller: AbortController; dispose?: AppDispose | void }>()

  const contextFor = (app: MemoryApp, controller: AbortController): AppServerContext => {
    const appId = app.manifest.id
    const own = new Map<string, (payload: unknown) => unknown>()
    handlers.set(appId, own)
    const storage = storages.get(appId) ?? new Map<string, unknown>()
    storages.set(appId, storage)
    return {
      appId,
      version: app.manifest.version,
      signal: controller.signal,
      dataDir: undefined,
      handle: (channel, handler) => void own.set(channel, handler),
      registerTool: (def) => {
        const id = qualifyTool(appId, def.name)
        tools.set(id, {
          info: { id, appId, name: def.name, title: def.title, description: def.description, readOnly: def.readOnly ?? false },
          def: def as unknown as AppToolDefinition
        })
      },
      emit: (event, payload) => appEvents.fire({ appId, event, payload }),
      openWindow: () => openRequests.fire(appId),
      storage: {
        get: <T>(key: string) => structuredClone(storage.get(key)) as T | undefined,
        set: (key, value) => void storage.set(key, structuredClone(value))
      },
      log: (...args) => console.debug(`[${appId}]`, ...args)
    }
  }

  for (const app of options.apps ?? []) {
    storages.set(app.manifest.id, new Map(Object.entries(app.storage ?? {})))
  }

  const installedApps = (): InstalledApp[] =>
    [...installed.keys()].map((id) => ({
      manifest: catalog.get(id)!.manifest,
      clientUrl: `memory://${id}`,
      source: `memory://${id}/bundle.json`,
      status: 'running' as const
    }))

  const register = async (app: MemoryApp): Promise<void> => {
    const controller = new AbortController()
    const entry: { controller: AbortController; dispose?: AppDispose | void } = { controller }
    installed.set(app.manifest.id, entry)
    if (app.server) entry.dispose = await app.server.setup(contextFor(app, controller))
  }

  const unregister = async (appId: string): Promise<void> => {
    const entry = installed.get(appId)
    if (!entry) return
    installed.delete(appId)
    entry.controller.abort()
    await entry.dispose?.()
    handlers.delete(appId)
    for (const [id, tool] of tools) if (tool.info.appId === appId) tools.delete(id)
  }

  const ready = (async () => {
    const ids = options.installed ?? [...catalog.keys()]
    for (const id of ids) {
      const app = catalog.get(id)
      if (!app) throw new Error(`createMemoryHost: installed app "${id}" is not in \`apps\``)
      await register(app)
    }
  })()

  const callTool = async (id: string, input: Record<string, unknown> = {}): Promise<ToolResult> => {
    await ready
    const tool = tools.get(id)
    if (!tool) throw new Error(`Unknown tool "${id}"`)
    const args = z.object(tool.def.input).parse(input)
    return tool.def.run(args)
  }

  // ── Agents ─────────────────────────────────────────────────────────────
  const agents = new Map((options.agents ?? []).map((a) => [a.id, a]))
  const running = new Map<string, { cancelled: boolean }>()
  const sessionConfig = new Map<string, { modes: SessionModeState | null; config: SessionConfigOption[] }>()
  const pendingPermissions = new Map<string, (optionId: string | null) => void>()
  let nextId = 0
  const newId = (prefix: string): string => `${prefix}-${++nextId}`

  const turnFor = (agentId: string, sessionId: string, state: { cancelled: boolean }): AgentTurn => {
    const push = async (update: Record<string, unknown>): Promise<void> => {
      await sleep(delay)
      updates.fire({ agentId, notification: { sessionId, update } as unknown as AgentUpdate['notification'] })
    }
    return {
      sessionId,
      get cancelled() {
        return state.cancelled
      },
      say: (text) => push({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } }),
      think: (text) => push({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text } }),
      async useTool(id, args = {}, title) {
        const toolCallId = newId('tool')
        await push({ sessionUpdate: 'tool_call', toolCallId, title: title ?? `mcp__agentos__${id}`, status: 'in_progress' })
        try {
          const result = await callTool(id, args)
          await push({ sessionUpdate: 'tool_call_update', toolCallId, status: 'completed' })
          return result
        } catch (error) {
          await push({ sessionUpdate: 'tool_call_update', toolCallId, status: 'failed' })
          throw error
        }
      },
      ask(title, opts) {
        const id = crypto.randomUUID()
        const options = opts ?? [
          { optionId: 'allow', name: 'Allow', kind: 'allow_once' },
          { optionId: 'reject', name: 'Deny', kind: 'reject_once' }
        ]
        return new Promise((resolve) => {
          pendingPermissions.set(id, resolve)
          permissions.fire({
            id,
            agentId,
            params: { sessionId, toolCall: { toolCallId: newId('tool'), title }, options } as unknown as AgentPermissionRequest['params']
          })
        })
      }
    }
  }

  const bridge: MemoryHost = {
    platform: options.platform ?? 'web',
    connection: { status: 'open', onStatus: () => () => {} },
    host: {
      info: async () => ({
        name: 'AgentOS (in-memory host)',
        version: '0.0.0',
        platform: options.platform ?? 'web',
        versions: { ...(options.versions ?? {}) }
      })
    },
    versions: Object.freeze({ ...(options.versions ?? {}) }),
    ready,
    calls,

    emit: (appId, event, payload) => appEvents.fire({ appId, event, payload }),
    storageOf: (appId) => storages.get(appId) ?? new Map(),

    window: {
      isFullScreen: async () => false,
      toggleFullScreen: () => record('window.toggleFullScreen', []),
      onFullScreenChange: () => () => {}
    },

    apps: {
      list: async () => {
        await ready
        return installedApps()
      },
      known: async () => {
        await ready
        return [...catalog.values()].map((a) => ({
          source: `memory://${a.manifest.id}/bundle.json`,
          manifest: a.manifest,
          installedVersion: installed.has(a.manifest.id) ? a.manifest.version : null
        }))
      },
      async install(appId) {
        record('apps.install', [appId])
        await ready
        const app = catalog.get(appId)
        if (!app) throw new Error(`Unknown app "${appId}"`)
        await unregister(appId)
        await register(app)
        changes.fire(installedApps())
        return installedApps().find((a) => a.manifest.id === appId)!
      },
      async uninstall(appId) {
        record('apps.uninstall', [appId])
        await ready
        await unregister(appId)
        changes.fire(installedApps())
      },
      onChanged: (listener) => changes.on(listener),
      async invoke(appId, channel, payload) {
        record('apps.invoke', [appId, channel, payload])
        await ready
        const handler = handlers.get(appId)?.get(channel)
        if (!handler) throw new Error(`App "${appId}" has no handler for "${channel}"`)
        return structuredClone(await handler(structuredClone(payload)))
      },
      onEvent: (listener) => appEvents.on(listener),
      onOpenRequest: (listener) => openRequests.on(listener)
    },

    tools: {
      list: async () => {
        await ready
        return [...tools.values()].map((t) => t.info)
      },
      call: (id, input) => {
        record('tools.call', [id, input])
        return callTool(id, input)
      }
    },

    agents: {
      list: async (): Promise<AgentSummary[]> =>
        [...agents.values()].map((a) => ({ id: a.id, name: a.name, running: true })),
      async newSession(agentId): Promise<AgentSessionInfo> {
        record('agents.newSession', [agentId])
        const agent = agents.get(agentId)
        if (!agent) throw new Error(`Unknown agent "${agentId}"`)
        await sleep(delay)
        const sessionId = newId('session')
        const state = {
          modes: agent.modes === undefined ? defaultScriptedModes() : agent.modes,
          config: structuredClone(agent.configOptions ?? defaultScriptedConfig())
        }
        sessionConfig.set(sessionId, state)
        return {
          sessionId,
          agentId,
          toolsConnected: agent.toolsConnected ?? true,
          modes: structuredClone(state.modes),
          configOptions: structuredClone(state.config)
        }
      },
      async prompt(agentId, sessionId, text) {
        record('agents.prompt', [agentId, sessionId, text])
        const agent = agents.get(agentId)
        if (!agent) throw new Error(`Unknown agent "${agentId}"`)
        const state = { cancelled: false }
        running.set(sessionId, state)
        try {
          const stop = await agent.respond(text, turnFor(agentId, sessionId, state))
          return state.cancelled ? 'cancelled' : (stop ?? 'end_turn')
        } finally {
          running.delete(sessionId)
        }
      },
      async cancel(agentId, sessionId) {
        record('agents.cancel', [agentId, sessionId])
        const state = running.get(sessionId)
        if (state) state.cancelled = true
      },
      async setMode(agentId, sessionId, modeId) {
        record('agents.setMode', [agentId, sessionId, modeId])
        const state = sessionConfig.get(sessionId)
        if (!state?.modes?.availableModes.some((m) => m.id === modeId)) throw new Error(`Unknown mode "${modeId}"`)
        state.modes.currentModeId = modeId
        updates.fire({
          agentId,
          notification: { sessionId, update: { sessionUpdate: 'current_mode_update', currentModeId: modeId } }
        })
      },
      async setConfig(agentId, sessionId, configId, value) {
        record('agents.setConfig', [agentId, sessionId, configId, value])
        const option = sessionConfig.get(sessionId)?.config.find((o) => o.id === configId)
        if (!option) throw new Error(`Unknown config option "${configId}"`)
        option.currentValue = value as never
        return structuredClone(sessionConfig.get(sessionId)!.config)
      },
      respondPermission(requestId, optionId) {
        record('agents.respondPermission', [requestId, optionId])
        pendingPermissions.get(requestId)?.(optionId)
        pendingPermissions.delete(requestId)
      },
      onUpdate: (listener) => updates.on(listener),
      onPermission: (listener) => permissions.on(listener),
      onExit: (listener) => exits.on(listener)
    }
  }
  return bridge
}

/** The scoped API an app module's component receives — for stories and component tests. */
export function appApiFor(
  host: HostBridge,
  appId: string,
  openApp: (appId: string) => void = () => {}
): AppClientApi {
  return createAppRuntime(host, { openApp }).apiFor(appId)
}

/** Install a host as `window.agentos`, the way the client installs its WebSocket bridge. */
export function installHost(host: HostBridge): void {
  Object.defineProperty(window, 'agentos', { value: host, configurable: true })
}

// ── A demo agent ─────────────────────────────────────────────────────────

const CALC_PROMPTS: [RegExp, string][] = [
  [/18% of 2,?450/i, '2450 * 0.18'],
  [/186\.40.*7 people/i, '186.40 / 7']
]

/**
 * A stand-in for Claude that behaves plausibly for the app's suggested
 * prompts: arithmetic goes to the Calculator's tools, history questions read
 * the calculation history, anything else gets a short markdown reply.
 */
export function demoAgent(overrides: Partial<ScriptedAgent> = {}): ScriptedAgent {
  return {
    id: 'claude',
    name: 'Claude',
    async respond(text, turn) {
      const calc = CALC_PROMPTS.find(([re]) => re.test(text))?.[1] ?? text.match(/^[\d\s.+\-*/%^()]+$/)?.[0]
      if (calc) {
        await turn.say('Let me work that out on the Calculator. ')
        const result = (await turn.useTool('calculator__show_on_calculator', { expression: calc })) as { shown: string }
        await turn.say(`The answer is **${result.shown}**. It's on the Calculator now.`)
        return 'end_turn'
      }
      if (/recent|history/i.test(text)) {
        const { entries } = (await turn.useTool('calculator__history', { limit: 5 })) as {
          entries: { expression: string; result: number }[]
        }
        if (!entries.length) {
          await turn.say("You haven't done any calculations yet.")
          return 'end_turn'
        }
        await turn.say('Your recent calculations:\n\n')
        for (const e of entries) await turn.say(`- \`${e.expression}\` = **${e.result}**\n`)
        return 'end_turn'
      }
      if (/delete|remove|send/i.test(text)) {
        const answer = await turn.ask(`Allow Claude to ${text.toLowerCase()}?`)
        await turn.say(answer === 'allow' ? 'Done.' : "OK, I won't do that.")
        return 'end_turn'
      }
      for (const word of `I can help with that. Here's a short answer to: *${text}*`.split(/(?<= )/)) {
        if (turn.cancelled) return 'cancelled'
        await turn.say(word)
      }
      return 'end_turn'
    },
    ...overrides
  }
}
