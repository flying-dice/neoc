/**
 * The AgentOS host protocol: the single source of truth for every message
 * between a client (the PWA) and the Bun server, carried as JSON-RPC 2.0 over
 * one WebSocket (`/rpc`). The JSON-RPC method is the channel name; params are
 * the positional args.
 *
 *  • Invoke  client → server, awaited          JSON-RPC request
 *  • Send    client → server, fire-and-forget  JSON-RPC notification
 *  • Event   server → every client             JSON-RPC notification
 *
 * Request types are declared explicitly; the zod schemas below are checked
 * against them at compile time and used by the server to validate everything a
 * client sends, so a buggy or hostile client can't pass bad input.
 */
import { z } from 'zod'
import type {
  RequestPermissionRequest,
  SessionConfigOption,
  SessionModeState,
  SessionNotification
} from '@agentclientprotocol/sdk'

export type { SessionConfigOption, SessionModeState } from '@agentclientprotocol/sdk'
import type { ToolInfo, ToolResult } from '../tools'
import type { AppManifest } from '../manifest'

// ── Shared payload types ─────────────────────────────────────────────────

export interface AgentSummary {
  id: string
  name: string
  running: boolean
}

export interface AgentSessionInfo {
  sessionId: string
  agentId: string
  /** Whether the agent accepted the AgentOS MCP server (needs HTTP MCP support). */
  toolsConnected: boolean
  /** Agent modes (e.g. plan / accept edits), when the agent offers them. */
  modes: SessionModeState | null
  /** Session settings the agent exposes: model, reasoning effort, … */
  configOptions: SessionConfigOption[]
}

/** New value for a session config option: a select value id, or a boolean toggle. */
export type ConfigValue = string | boolean

export interface AppEvent {
  appId: string
  event: string
  payload: unknown
}

export interface AgentUpdate {
  agentId: string
  notification: SessionNotification
}

export interface AgentPermissionRequest {
  /** Correlates the user's answer (`agents.respondPermission`). */
  id: string
  agentId: string
  params: RequestPermissionRequest
}

export interface AgentExit {
  agentId: string
  code: number | null
}

export interface HostInfo {
  name: string
  version: string
  /** Server OS (e.g. "darwin", "linux"). */
  platform: string
  /** Runtime versions on the server (bun, …). */
  versions: Record<string, string>
}

/** An app installed on this server, as clients see it. */
export interface InstalledApp {
  manifest: AppManifest
  /** Where the client bundle is served (an ESM module, imported on first open). */
  clientUrl: string
  /** Registry it came from. */
  registry: string
  /** "running" once its server module is set up; "failed" if setup threw. */
  status: 'running' | 'failed'
  error?: string
}

/** An app offered by a registry. */
export interface RegistryApp {
  registry: string
  manifest: AppManifest
  /** Version installed here, if any (compare with manifest.version for updates). */
  installedVersion: string | null
}

// ── Contracts ────────────────────────────────────────────────────────────

export interface InvokeContract {
  'host:info': { args: []; result: HostInfo }
  'apps:list': { args: []; result: InstalledApp[] }
  'apps:install': { args: [registry: string, appId: string]; result: InstalledApp }
  'apps:uninstall': { args: [appId: string]; result: void }
  'registry:list': { args: []; result: RegistryApp[] }
  'apps:invoke': { args: [appId: string, channel: string, payload: unknown]; result: unknown }
  'tools:list': { args: []; result: ToolInfo[] }
  'tools:call': { args: [id: string, input: Record<string, unknown>]; result: ToolResult }
  'agents:list': { args: []; result: AgentSummary[] }
  'agents:new-session': { args: [agentId: string]; result: AgentSessionInfo }
  'agents:prompt': { args: [agentId: string, sessionId: string, text: string]; result: string }
  'agents:cancel': { args: [agentId: string, sessionId: string]; result: void }
  'agents:set-mode': { args: [agentId: string, sessionId: string, modeId: string]; result: void }
  'agents:set-config': {
    args: [agentId: string, sessionId: string, configId: string, value: ConfigValue]
    result: SessionConfigOption[]
  }
}

export interface SendContract {
  'agents:permission-response': [id: string, optionId: string | null]
}

export interface EventContract {
  /** The installed app set changed (install, uninstall, update, reload). */
  'apps:changed': { apps: InstalledApp[] }
  'apps:event': AppEvent
  'apps:open': { appId: string }
  'agents:update': AgentUpdate
  'agents:permission': AgentPermissionRequest
  'agents:exit': AgentExit
}

export type InvokeChannel = keyof InvokeContract
export type SendChannel = keyof SendContract
export type EventChannel = keyof EventContract

// ── Runtime validation for client input ──────────────────────────────────

const id = z.string().min(1).max(200)
const appId = z.string().regex(/^[a-z][a-z0-9-]{1,31}$/)
const registry = z.string().min(1).max(100)

type Schemas<T> = { [C in keyof T]: z.ZodType<T[C]> }

export const invokeSchemas = {
  'host:info': z.tuple([]),
  'apps:list': z.tuple([]),
  'apps:install': z.tuple([registry, appId]),
  'apps:uninstall': z.tuple([appId]),
  'registry:list': z.tuple([]),
  'apps:invoke': z.tuple([appId, z.string().min(1).max(100), z.unknown()]),
  'tools:list': z.tuple([]),
  'tools:call': z.tuple([z.string().regex(/^[A-Za-z0-9_-]{1,80}$/), z.record(z.string(), z.unknown())]),
  'agents:list': z.tuple([]),
  'agents:new-session': z.tuple([id]),
  'agents:prompt': z.tuple([id, id, z.string().min(1).max(100_000)]),
  'agents:cancel': z.tuple([id, id]),
  'agents:set-mode': z.tuple([id, id, id]),
  'agents:set-config': z.tuple([id, id, id, z.union([z.string().min(1).max(200), z.boolean()])])
} satisfies Schemas<{ [C in InvokeChannel]: InvokeContract[C]['args'] }>

export const sendSchemas = {
  'agents:permission-response': z.tuple([z.string().uuid(), id.nullable()])
} satisfies Schemas<SendContract>

export const invokeChannels = Object.keys(invokeSchemas) as InvokeChannel[]
export const sendChannels = Object.keys(sendSchemas) as SendChannel[]
