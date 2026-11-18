/**
 * Install the in-memory host before any store is imported (stores read the
 * bridge on first import), so built-in app stories run against the same
 * fake backend as web mode: real app server modules, a scripted Claude, and
 * repo apps' clients loaded from source.
 */
import '../src/renderer/src/lib/sharedModules'
import { createMemoryHost, demoAgent, installHost } from '@agentos/sdk/testing'
import { repoApps } from '../src/renderer/src/web/repoApps'

const host = createMemoryHost({
  apps: repoApps(),
  agents: [demoAgent()],
  versions: { client: 'storybook', chrome: '146.0.7680.65' }
})
installHost(host)

// Stories render the Dock: give them the installed set before the first story mounts.
await host.ready
const { setInstalledApps } = await import('../src/renderer/src/lib/apps.svelte')
setInstalledApps(await host.apps.list())
