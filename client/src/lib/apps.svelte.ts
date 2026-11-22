import type { IconName } from '@vos/ui'
import { iconOf, tintBackground, type AppSummary } from '../shell/hostApi'

/** Apps whose pages still ship with the shell. */
export type BuiltinAppId = 'discover' | 'settings' | 'files' | 'terminal'

/** Built-in ids plus the ids of the Lua apps installed on the host. */
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
export const MIN_WIDTH = 320
const MIN_HEIGHT = 240

/** Registry of the built-in pages. They are wired to ids in App.svelte. */
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

const isBuiltin = (id: string): boolean => id in builtinApps

/** An installed app's window and look, from what the app declared (`app.manifest`). */
const toDefinition = (a: AppSummary): AppDefinition => ({
  id: a.id,
  name: a.name,
  icon: iconOf(a.icon),
  background: tintBackground(a.tint),
  width: a.window?.w ?? 720,
  height: a.window?.h ?? 520,
  minWidth: a.window?.minW ?? MIN_WIDTH,
  minHeight: a.window?.minH ?? MIN_HEIGHT
})

/**
 * Every app the shell can open a window for: the built-in pages plus the installed apps that are running.
 * Reactive: installs, uninstalls, disables and failures (from any client) arrive as `app.*` notifications and
 * update it in place.
 */
export const apps: Record<AppId, AppDefinition> = $state({ ...builtinApps })

const installed = $state<{ list: AppSummary[] }>({ list: [] })

/**
 * The definition of an app the shell can open a window for, or `undefined` (not installed, disabled, failed, or not
 * an id at all). Reads through `apps`, so a template that asks re-renders when the app comes or goes.
 */
export function appOf(id: AppId): AppDefinition | undefined {
  const definition = apps[id]
  // `apps` is a plain record: `constructor` or `__proto__` must not find an inherited member
  return definition?.id === id ? definition : undefined
}

/** An installed app by id, in any state. */
export const installedApp = (id: AppId): AppSummary | undefined =>
  installed.list.find((a) => a.id === id)

type RemovedListener = (appIds: AppId[]) => void
/** Plain (non-reactive) listener list: callbacks, not state. */
let removedListeners: RemovedListener[] = []

/**
 * Called synchronously, before apps that stopped running (uninstalled, disabled, failed) leave `apps`, so
 * their windows can close while their definitions still exist (nothing renders a window for an app that is
 * already gone).
 */
export function onAppsRemoved(listener: RemovedListener): () => void {
  removedListeners = [...removedListeners, listener]
  return () => {
    removedListeners = removedListeners.filter((l) => l !== listener)
  }
}

export function setInstalledApps(list: AppSummary[]): void {
  const running = list.filter((a) => a.state === 'running')
  const next = running.map((a) => a.id)
  const removed = Object.keys(apps).filter((id) => !isBuiltin(id) && !next.includes(id))
  if (removed.length) removedListeners.forEach((l) => l(removed))
  for (const id of removed) delete apps[id]
  for (const app of running) if (!isBuiltin(app.id)) apps[app.id] = toDefinition(app)
  installed.list = list
}

/**
 * Dock: the built-ins, then the running installed apps, then Settings.
 */
export function dockOrder(): AppId[] {
  return [
    'discover',
    'files',
    'terminal',
    ...installed.list.filter((a) => a.state === 'running' && !isBuiltin(a.id)).map((a) => a.id),
    'settings'
  ]
}
