/**
 * @agentos/sdk — shared contracts.
 *
 *   @agentos/sdk               manifest + tool types (both sides, both processes)
 *   @agentos/sdk/app/main      for app modules' main entry   (defineMainApp)
 *   @agentos/sdk/app/renderer  for app modules' UI           (defineRendererApp, useAppApi)
 *   @agentos/sdk/host/protocol the IPC contract (channels, payloads, validation)
 *   @agentos/sdk/host/main     host main process   (registerHostIpc, createHostEmitter)
 *   @agentos/sdk/host/preload  host preload        (createHostBridge)
 *   @agentos/sdk/host/renderer host renderer       (hostBridge, createAppRuntime)
 */
export { defineManifest, type AppManifest } from './manifest'
export { qualifyTool, type AppToolDefinition, type ToolInfo, type ToolResult } from './tools'
