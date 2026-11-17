/**
 * Preload side: builds the `HostBridge` over `ipcRenderer`, using only the
 * channels in the protocol. Runs in a sandboxed preload — no Node APIs.
 *
 *   contextBridge.exposeInMainWorld(BRIDGE_GLOBAL, createHostBridge(ipcRenderer, process))
 */
import type { IpcRenderer, IpcRendererEvent } from 'electron'
import type { HostBridge, Unsubscribe } from './bridge'
import {
  wire,
  type EventChannel,
  type EventContract,
  type InvokeChannel,
  type InvokeContract,
  type SendChannel,
  type SendContract
} from './protocol'

export { BRIDGE_GLOBAL } from './bridge'

export function createHostBridge(
  ipc: IpcRenderer,
  runtime: { platform: string; versions: Record<string, string | undefined> }
): HostBridge {
  const invoke = <C extends InvokeChannel>(
    channel: C,
    ...args: InvokeContract[C]['args']
  ): Promise<InvokeContract[C]['result']> => ipc.invoke(wire(channel), ...args)

  const send = <C extends SendChannel>(channel: C, ...args: SendContract[C]): void =>
    ipc.send(wire(channel), ...args)

  const on = <C extends EventChannel>(channel: C, listener: (payload: EventContract[C]) => void): Unsubscribe => {
    const handler = (_: IpcRendererEvent, payload: EventContract[C]): void => listener(payload)
    ipc.on(wire(channel), handler)
    return () => ipc.removeListener(wire(channel), handler)
  }

  const versions = Object.freeze(
    Object.fromEntries(
      Object.entries(runtime.versions).filter((e): e is [string, string] => typeof e[1] === 'string')
    )
  )

  return {
    platform: runtime.platform,
    versions,
    window: {
      isFullScreen: () => invoke('window:is-fullscreen'),
      toggleFullScreen: () => send('window:toggle-fullscreen'),
      onFullScreenChange: (listener) => on('window:fullscreen-changed', (e) => listener(e.fullScreen)),
      quit: () => send('app:quit')
    },
    apps: {
      invoke: (appId, channel, payload) => invoke('apps:invoke', appId, channel, payload),
      onEvent: (listener) => on('apps:event', listener),
      onOpenRequest: (listener) => on('apps:open', (e) => listener(e.appId))
    },
    tools: {
      list: () => invoke('tools:list'),
      call: (id, input = {}) => invoke('tools:call', id, input)
    },
    agents: {
      list: () => invoke('agents:list'),
      newSession: (agentId) => invoke('agents:new-session', agentId),
      prompt: (agentId, sessionId, text) => invoke('agents:prompt', agentId, sessionId, text),
      cancel: (agentId, sessionId) => invoke('agents:cancel', agentId, sessionId),
      setMode: (agentId, sessionId, modeId) => invoke('agents:set-mode', agentId, sessionId, modeId),
      setConfig: (agentId, sessionId, configId, value) =>
        invoke('agents:set-config', agentId, sessionId, configId, value),
      respondPermission: (requestId, optionId) => send('agents:permission-response', requestId, optionId),
      onUpdate: (listener) => on('agents:update', listener),
      onPermission: (listener) => on('agents:permission', listener),
      onExit: (listener) => on('agents:exit', listener)
    }
  }
}
