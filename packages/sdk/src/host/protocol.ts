/**
 * The AgentOS host IPC protocol — the single source of truth for every
 * message between the host renderer (via the preload bridge) and main.
 *
 *  • Invoke  renderer → main, awaited       (`ipcRenderer.invoke` / `ipcMain.handle`)
 *  • Send    renderer → main, fire-and-forget (`ipcRenderer.send` / `ipcMain.on`)
 *  • Event   main → renderer                 (`webContents.send` / `ipcRenderer.on`)
 *
 * Request types are declared explicitly; the zod schemas below are checked
 * against them at compile time and used by main to validate everything the
 * renderer sends, so a compromised or buggy renderer can't pass bad input.
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

// ── Contracts ────────────────────────────────────────────────────────────

export interface InvokeContract {
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
  'window:is-fullscreen': { args: []; result: boolean }
}

export interface SendContract {
  'window:toggle-fullscreen': []
  'app:quit': []
  'agents:permission-response': [id: string, optionId: string | null]
}

export interface EventContract {
  'apps:event': AppEvent
  'apps:open': { appId: string }
  'agents:update': AgentUpdate
  'agents:permission': AgentPermissionRequest
  'agents:exit': AgentExit
  'window:fullscreen-changed': { fullScreen: boolean }
}

export type InvokeChannel = keyof InvokeContract
export type SendChannel = keyof SendContract
export type EventChannel = keyof EventContract

/** Wire name for a channel — namespaced so nothing else on IPC can collide. */
export const wire = (channel: InvokeChannel | SendChannel | EventChannel): string => `agentos:${channel}`

// ── Runtime validation for renderer input ────────────────────────────────

const id = z.string().min(1).max(200)
const appId = z.string().regex(/^[a-z][a-z0-9-]{1,31}$/)

type Schemas<T> = { [C in keyof T]: z.ZodType<T[C]> }

export const invokeSchemas = {
  'apps:invoke': z.tuple([appId, z.string().min(1).max(100), z.unknown()]),
  'tools:list': z.tuple([]),
  'tools:call': z.tuple([z.string().regex(/^[A-Za-z0-9_-]{1,80}$/), z.record(z.string(), z.unknown())]),
  'agents:list': z.tuple([]),
  'agents:new-session': z.tuple([id]),
  'agents:prompt': z.tuple([id, id, z.string().min(1).max(100_000)]),
  'agents:cancel': z.tuple([id, id]),
  'agents:set-mode': z.tuple([id, id, id]),
  'agents:set-config': z.tuple([id, id, id, z.union([z.string().min(1).max(200), z.boolean()])]),
  'window:is-fullscreen': z.tuple([])
} satisfies Schemas<{ [C in InvokeChannel]: InvokeContract[C]['args'] }>

export const sendSchemas = {
  'window:toggle-fullscreen': z.tuple([]),
  'app:quit': z.tuple([]),
  'agents:permission-response': z.tuple([z.string().uuid(), id.nullable()])
} satisfies Schemas<SendContract>

export const invokeChannels = Object.keys(invokeSchemas) as InvokeChannel[]
export const sendChannels = Object.keys(sendSchemas) as SendChannel[]
