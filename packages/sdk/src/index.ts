/**
 * @agentos/sdk: shared contracts.
 *
 *   @agentos/sdk               manifest + tool types (everywhere)
 *   @agentos/sdk/app/server    app server module (defineServerApp, AppServerContext)
 *   @agentos/sdk/app/client    app client UI (defineClientApp, useAppApi)
 *   @agentos/sdk/host/protocol the client ↔ server contract (channels, payloads, zod validation)
 *   @agentos/sdk/host/server   server-side dispatcher (createProtocolDispatcher, HostEmitter)
 *   @agentos/sdk/host/client   client-side bridge (createWsHostBridge, hostBridge, createAppRuntime)
 *   @agentos/sdk/rpc           JSON-RPC 2.0 peer (the wire)
 *   @agentos/sdk/registry      registry.json schema
 *   @agentos/sdk/build         app bundle build (Bun only)
 *   @agentos/sdk/testing       in-memory host for headless tests and web mode
 */
export { defineManifest, type AppManifest } from './manifest'
export { qualifyTool, type AppToolDefinition, type ToolInfo, type ToolResult } from './tools'
