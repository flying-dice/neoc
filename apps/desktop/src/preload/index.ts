/**
 * Sandboxed preload: exposes exactly one frozen global, `window.agentos`,
 * implementing the SDK's `HostBridge` over the typed host protocol.
 */
import { contextBridge, ipcRenderer } from 'electron'
import { BRIDGE_GLOBAL, createHostBridge } from '@agentos/sdk/host/preload'

contextBridge.exposeInMainWorld(
  BRIDGE_GLOBAL,
  createHostBridge(ipcRenderer, { platform: process.platform, versions: process.versions })
)
