import type { IconName } from '@agentos/ui'
import { hostBridge, type InstalledApp } from '@agentos/sdk/host/client'

/** Apps whose pages still ship with the host (to be migrated to modules). */
export type BuiltinAppId = 'discover' | 'agents' | 'settings' | 'files' | 'gallery' | 'terminal'

/** Built-in ids plus the ids of app modules installed on the server. */
export type AppId = string

export interface AppDefinition {
  id: AppId
  name: string
  icon: IconName
  background: string
  width: number
  height: number
  /** Smallest floating window the app's layout supports. */
  minWidth: number
  minHeight: number
  /** Native-macOS layout (translucent sidebar) rather than a Catalyst-style iPad page. */
  vibrant?: boolean
}

/** The smallest window any app gets (iPadOS Stage Manager's minimum is similar). */
const MIN_WIDTH = 320
const MIN_HEIGHT = 240

/** Registry of built-in AgentOS apps. Pages are wired to ids in App.svelte. */
const builtinApps: Record<BuiltinAppId, AppDefinition> = {
  discover: {
    id: 'discover',
    name: 'Discover',
    icon: 'star',
    background: 'linear-gradient(180deg, #4fc3ff 0%, #0a6cff 100%)',
    width: 1020,
    height: 640,
    minWidth: MIN_WIDTH,
    minHeight: MIN_HEIGHT,
    vibrant: true
  },
  agents: {
    id: 'agents',
    name: 'Agents',
    icon: 'sparkles',
    background: 'linear-gradient(180deg, #a78bfa 0%, #6d28d9 100%)',
    width: 920,
    height: 600,
    minWidth: MIN_WIDTH,
    minHeight: MIN_HEIGHT
  },
  settings: {
    id: 'settings',
    name: 'Settings',
    icon: 'gear',
    background: 'linear-gradient(180deg, #a1a1aa 0%, #52525b 100%)',
    width: 860,
    height: 580,
    minWidth: MIN_WIDTH,
    minHeight: MIN_HEIGHT
  },
  files: {
    id: 'files',
    name: 'Files',
    icon: 'folder',
    background: 'linear-gradient(180deg, #5ac8fa 0%, #007aff 100%)',
    width: 640,
    height: 520,
    minWidth: MIN_WIDTH,
    minHeight: MIN_HEIGHT
  },
  gallery: {
    id: 'gallery',
    name: 'UI Kit',
    icon: 'paintbrush',
    background: 'linear-gradient(180deg, #ffb340 0%, #ff6a00 100%)',
    width: 880,
    height: 640,
    minWidth: MIN_WIDTH,
    minHeight: MIN_HEIGHT
  },
  terminal: {
    id: 'terminal',
    name: 'Terminal',
    icon: 'terminal',
    background: 'linear-gradient(180deg, #3a3a3c 0%, #000000 100%)',
    width: 640,
    height: 420,
    minWidth: MIN_WIDTH,
    minHeight: MIN_HEIGHT
  }
}

const toDefinition = ({ manifest: m }: InstalledApp): AppDefinition => ({
  id: m.id,
  name: m.name,
  icon: m.icon,
  background: m.background,
  width: m.window?.width ?? 720,
  height: m.window?.height ?? 520,
  minWidth: m.window?.minWidth ?? MIN_WIDTH,
  minHeight: m.window?.minHeight ?? MIN_HEIGHT
})

/**
 * Every app the shell can open: the built-ins plus the app modules installed
 * on the server. Reactive: installs, uninstalls and reloads (from any client)
 * arrive as `apps:changed` and update it in place.
 */
export const apps: Record<AppId, AppDefinition> = $state({ ...builtinApps })

const installed = $state<{ list: InstalledApp[] }>({ list: [] })

/** App modules currently installed on the server. */
export const installedApps = {
  get list(): InstalledApp[] {
    return installed.list
  }
}

export const moduleApp = (id: AppId): InstalledApp | undefined =>
  installed.list.find((a) => a.manifest.id === id)

type RemovedListener = (appIds: AppId[]) => void
/** Plain (non-reactive) listener list: callbacks, not state. */
let removedListeners: RemovedListener[] = []

/**
 * Called synchronously, before uninstalled apps leave `apps`, so their windows
 * can close while their definitions still exist (nothing renders a window for
 * an app that's already gone).
 */
export function onAppsRemoved(listener: RemovedListener): () => void {
  removedListeners = [...removedListeners, listener]
  return () => {
    removedListeners = removedListeners.filter((l) => l !== listener)
  }
}

export function setInstalledApps(list: InstalledApp[]): void {
  const next = list.map((a) => a.manifest.id)
  const removed = installed.list
    .map((a) => a.manifest.id)
    .filter((id) => !next.includes(id) && !(id in builtinApps))
  if (removed.length) removedListeners.forEach((l) => l(removed))
  for (const old of installed.list)
    if (!(old.manifest.id in builtinApps)) delete apps[old.manifest.id]
  for (const app of list)
    if (!(app.manifest.id in builtinApps)) apps[app.manifest.id] = toDefinition(app)
  installed.list = list
}

/** Dock: built-ins, then installed apps that ask for a Dock slot, then Settings. */
export function dockOrder(): AppId[] {
  return [
    'discover',
    'agents',
    'files',
    'terminal',
    ...installed.list
      .filter((a) => a.manifest.dock && a.status === 'running')
      .map((a) => a.manifest.id),
    'settings'
  ]
}
export const dockRecents: AppId[] = ['gallery']

// Keep the installed set in sync with the server (initial load, live changes, reconnects).
const host = hostBridge()
if (host) {
  const refresh = (): void =>
    void host.apps.list().then(setInstalledApps, (e: unknown) => console.warn('[agentos] apps:', e))
  refresh()
  host.apps.onChanged(setInstalledApps)
  host.connection.onStatus((status) => status === 'open' && refresh())
}
