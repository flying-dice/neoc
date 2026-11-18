/**
 * `HostBridge`: the complete API the AgentOS client (the PWA shell) may use,
 * installed as the frozen global `window.agentos`. In production it is the
 * WebSocket JSON-RPC bridge to the Bun server (`createWsHostBridge`); in tests
 * and web mode it is the in-memory host (`@agentos/sdk/testing`). Declared here,
 * not inferred from either implementation, so all of them compile against it.
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
  HostInfo,
  InstalledApp,
  KnownApp,
  SessionConfigOption
} from './protocol'

export type Unsubscribe = () => void

/** WebSocket link state; the in-memory host is always "open". */
export type ConnectionStatus = 'connecting' | 'open' | 'reconnecting' | 'closed'

export interface HostBridge {
  /** Host OS, e.g. "darwin", "win32", "linux". */
  readonly platform: string
  /** Runtime versions (electron, chrome, node…). */
  readonly versions: Readonly<Record<string, string>>

  /** The connection to the server. */
  connection: {
    readonly status: ConnectionStatus
    onStatus(listener: (status: ConnectionStatus) => void): Unsubscribe
  }

  /** The server this client is attached to. */
  host: {
    info(): Promise<HostInfo>
  }

  /** The browser window the shell runs in (client-local, no server round trip). */
  window: {
    isFullScreen(): Promise<boolean>
    toggleFullScreen(): void
    onFullScreenChange(listener: (fullScreen: boolean) => void): Unsubscribe
  }

  /** Installed app modules: lifecycle, client ↔ server calls and events, namespaced by app id. */
  apps: {
    list(): Promise<InstalledApp[]>
    /** Apps the server knows how to install, with what's installed. */
    known(): Promise<KnownApp[]>
    /** Install or update a known app. */
    install(appId: string): Promise<InstalledApp>
    uninstall(appId: string): Promise<void>
    /** The installed set changed (any client installed/uninstalled, or the server reloaded an app). */
    onChanged(listener: (apps: InstalledApp[]) => void): Unsubscribe
    invoke(appId: string, channel: string, payload?: unknown): Promise<unknown>
    onEvent(listener: (event: AppEvent) => void): Unsubscribe
    /** An app's server module asked to open its window. */
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

/** The global the shell installs the bridge as. */
export const BRIDGE_GLOBAL = 'agentos'

/** Install a bridge as `window.agentos` (once, non-writable). */
export function installHostBridge(bridge: HostBridge): void {
  Object.defineProperty(globalThis, BRIDGE_GLOBAL, { value: Object.freeze(bridge), configurable: false, writable: false })
}

declare global {
  interface Window {
    /** Present once the shell has installed its host bridge. */
    readonly agentos?: HostBridge
  }
}
