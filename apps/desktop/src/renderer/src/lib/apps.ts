import type { IconName } from '@agentos/ui'
import { installedApps } from './installedApps'

/** Apps whose pages still ship with the host (to be migrated to modules). */
export type BuiltinAppId = 'discover' | 'agents' | 'settings' | 'files' | 'gallery' | 'terminal'

/** Built-in ids plus the ids of installed app modules. */
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

/** Installed app modules, from their manifests. */
const moduleApps: Record<string, AppDefinition> = Object.fromEntries(
  installedApps.map(({ manifest: m }) => [
    m.id,
    {
      id: m.id,
      name: m.name,
      icon: m.icon,
      background: m.background,
      width: m.window?.width ?? 720,
      height: m.window?.height ?? 520,
      minWidth: m.window?.minWidth ?? MIN_WIDTH,
      minHeight: m.window?.minHeight ?? MIN_HEIGHT
    }
  ])
)

export const apps: Record<AppId, AppDefinition> = { ...builtinApps, ...moduleApps }

export const dockOrder: AppId[] = [
  'discover',
  'agents',
  'files',
  'terminal',
  ...installedApps.filter((a) => a.manifest.dock).map((a) => a.manifest.id),
  'settings'
]
export const dockRecents: AppId[] = ['gallery']
