import { createAppRuntime, hostBridge } from '@agentos/sdk/host/client'
import { windowManager } from './windowManager.svelte'

/**
 * The shell's app runtime: hands each app module its scoped SDK API. Created
 * at startup so server → app events are captured (and queued) before any app
 * window mounts.
 */
export const appRuntime = createAppRuntime(hostBridge(), {
  openApp: (appId) => windowManager.open(appId)
})
