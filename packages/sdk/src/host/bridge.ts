/**
 * `HostBridge` — the complete API the host renderer may use, exposed by the
 * preload as the frozen global `window.agentos`. Declared here (not inferred
 * from the preload) so both sides compile against the same contract.
 */
import type { ToolInfo, ToolResult } from '../tools'
import type {
  AgentExit,
  AgentPermissionRequest,
  AgentSessionInfo,
  AgentSummary,
  AgentUpdate,
  AppEvent,
  ConfigValue,
  SessionConfigOption
} from './protocol'

export type Unsubscribe = () => void

export interface HostBridge {
  /** Host OS, e.g. "darwin", "win32", "linux". */
  readonly platform: string
  /** Runtime versions (electron, chrome, node…). */
  readonly versions: Readonly<Record<string, string>>

  window: {
    isFullScreen(): Promise<boolean>
    toggleFullScreen(): void
    onFullScreenChange(listener: (fullScreen: boolean) => void): Unsubscribe
    quit(): void
  }

  /** App modules: renderer ↔ main calls and events, namespaced by app id. */
  apps: {
    invoke(appId: string, channel: string, payload?: unknown): Promise<unknown>
    onEvent(listener: (event: AppEvent) => void): Unsubscribe
    /** An app's main module asked to open its window. */
    onOpenRequest(listener: (appId: string) => void): Unsubscribe
  }

  /** The universal tool registry (every app's tools). */
  tools: {
    list(): Promise<ToolInfo[]>
    call(id: string, input?: Record<string, unknown>): Promise<ToolResult>
  }

  /** ACP agents run by the main process. */
  agents: {
    list(): Promise<AgentSummary[]>
    newSession(agentId: string): Promise<AgentSessionInfo>
    prompt(agentId: string, sessionId: string, text: string): Promise<string>
    cancel(agentId: string, sessionId: string): Promise<void>
    /** Switch the session's mode (ACP `session/set_mode`). */
    setMode(agentId: string, sessionId: string, modeId: string): Promise<void>
    /** Change a session setting such as model or effort; returns all options. */
    setConfig(agentId: string, sessionId: string, configId: string, value: ConfigValue): Promise<SessionConfigOption[]>
    respondPermission(requestId: string, optionId: string | null): void
    onUpdate(listener: (update: AgentUpdate) => void): Unsubscribe
    onPermission(listener: (request: AgentPermissionRequest) => void): Unsubscribe
    onExit(listener: (exit: AgentExit) => void): Unsubscribe
  }
}

/** The global the preload defines. */
export const BRIDGE_GLOBAL = 'agentos'

declare global {
  interface Window {
    /** Present only inside the AgentOS desktop host. */
    readonly agentos?: HostBridge
  }
}
