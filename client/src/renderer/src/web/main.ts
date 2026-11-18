/**
 * Web-mode entry (`web.html`): the real shell on the in-memory host. No
 * server: app server modules run in the page and app clients load from source.
 *
 *   ?open=calculator,settings   open these apps instead of Discover
 *   ?theme=light|dark           force the appearance
 *   ?scenario=agents-busy       seed state (see ./scenarios.ts)
 *   ?delay=30                   ms between streamed agent chunks
 *
 * Exposes `window.__agentos` (host + stores) for tests and the research panel.
 */
import '../lib/sharedModules'
import { createMemoryHost, demoAgent, installHost, type MemoryHost } from '@agentos/sdk/testing'
import { repoApps } from './repoApps'
import { scenarios } from './scenarios'

const params = new URLSearchParams(location.search)

const host = createMemoryHost({
  // Every repo app, run from source (the server runs their built bundles instead).
  apps: repoApps(),
  agents: [demoAgent()],
  streamDelay: Number(params.get('delay') ?? 30),
  versions: {
    client: 'web mode',
    chrome: navigator.userAgent.match(/Chrome\/([\d.]+)/)?.[1] ?? '—'
  }
})
installHost(host)

// Stores read the bridge when first imported, so import them after installing it.
const [{ windowManager }, { settings }, { agentStore }, { apps }] = await Promise.all([
  import('../lib/windowManager.svelte'),
  import('../lib/settings.svelte'),
  import('../lib/agents.svelte'),
  import('../lib/apps.svelte')
])

const theme = params.get('theme')
if (theme === 'light' || theme === 'dark') settings.theme = theme

// The installed set arrives asynchronously from the host; let it land first.
await host.ready
const { setInstalledApps } = await import('../lib/apps.svelte')
setInstalledApps(await host.apps.list())

await import('../main')

const scenario = params.get('scenario')
if (scenario) {
  const seed = scenarios[scenario]
  if (!seed)
    throw new Error(`Unknown scenario "${scenario}". Known: ${Object.keys(scenarios).join(', ')}`)
  seed({ agentStore })
}

// Let the desktop measure its final workspace (after styles load) so windows
// cascade from the real bounds and every run lays out identically.
const frame = (): Promise<unknown> => new Promise((r) => requestAnimationFrame(r))
for (let i = 0; i < 60 && windowManager.bounds.width !== document.documentElement.clientWidth; i++)
  await frame()
await frame()

const open = params.get('open')
if (open) {
  for (const win of [...windowManager.windows]) windowManager.close(win.id)
  for (const id of open.split(',').filter(Boolean)) {
    if (!apps[id]) throw new Error(`Unknown app "${id}". Known: ${Object.keys(apps).join(', ')}`)
    windowManager.open(id)
  }
}

declare global {
  interface Window {
    __agentos: {
      host: MemoryHost
      wm: typeof windowManager
      settings: typeof settings
      agentStore: typeof agentStore
      apps: typeof apps
    }
  }
}
window.__agentos = { host, wm: windowManager, settings, agentStore, apps }
document.documentElement.dataset.ready = 'true'
