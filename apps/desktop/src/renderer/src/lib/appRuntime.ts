import { createAppRuntime, hostBridge } from '@agentos/sdk/host/renderer'
import { windowManager } from './windowManager.svelte'

/**
 * The host's app runtime: hands each app module its scoped SDK API. Created
 * at startup so main → app events are captured (and queued) before any app
 * window mounts.
 */
export const appRuntime = createAppRuntime(hostBridge(), {
  openApp: (appId) => windowManager.open(appId)
})
