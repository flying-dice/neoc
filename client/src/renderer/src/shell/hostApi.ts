/**
 * What the shell asks the host besides the view protocol: the app lifecycle (`app.*`), the catalog Discover
 * browses and the paired devices (`host.devices.*`). The shapes mirror `crates/agentos-domain/src/api.rs`
 * (generated into `types/generated/agentos.ts`; `hostApi.test.ts` fails if the two disagree).
 */
import { icons, type IconName } from '@agentos/ui'

export type AppState = 'running' | 'disabled' | 'failed'

export interface AppWindowDecl {
  w: number
  h: number
  minW: number
  minH: number
}

export interface AppSummary {
  id: string
  name: string
  version: string
  state: AppState
  message?: string
  icon?: string
  tint?: string
  description?: string
  window?: AppWindowDecl
  source: string
}

export interface InstalledInfo {
  version: string
  state: AppState
}

export interface AppPreview {
  url: string
  id: string
  name: string
  version: string
  icon?: string
  tint?: string
  description?: string
  permissions: string[]
  source: string
  installed?: InstalledInfo
}

export interface Catalog {
  apps: AppPreview[]
  errors: string[]
}

/** One agent on the host's roster (`agents.list`; grants and tokens never come with it). */
export interface AgentRow {
  id: string
  name: string
  avatar?: string
  role: string
}

export interface DeviceRow {
  id: string
  name: string
  createdAt: number
  lastSeenAt: number
  current: boolean
}

/** The calls the shell makes. The real one is the session's `call`; stories and tests pass a fake. */
export interface HostApi {
  call<T>(method: string, params?: Record<string, unknown>): Promise<T>
  /** Host notifications that no window owns (`window.open`, `app.installed`, ...). */
  onNotification(listener: (method: string, params: Record<string, unknown>) => void): () => void
}

/** The icon of an app: its declared symbol when the kit has it, else a generic one. */
export function iconOf(name: string | undefined): IconName {
  return name && name in icons ? (name as IconName) : 'square.grid'
}

/** Palette roles an app may tint its icon with, as the kit's colour variables. */
const TINTS: Record<string, string> = {
  red: 'var(--ios-red)',
  orange: 'var(--ios-orange)',
  yellow: 'var(--ios-yellow)',
  green: 'var(--ios-green)',
  mint: 'var(--ios-mint, var(--ios-teal))',
  teal: 'var(--ios-teal)',
  cyan: 'var(--ios-cyan, var(--ios-teal))',
  blue: 'var(--ios-blue)',
  indigo: 'var(--ios-indigo, var(--ios-purple))',
  purple: 'var(--ios-purple)',
  pink: 'var(--ios-pink)',
  gray: 'var(--ios-gray)'
}

/** The Dock and library background for an app's tint (two stops, like the built-in apps). */
export function tintBackground(tint: string | undefined): string {
  const colour = TINTS[tint ?? ''] ?? TINTS.blue
  return `linear-gradient(180deg, color-mix(in srgb, ${colour} 78%, #fff), ${colour})`
}

/** `3 minutes ago`-style label for a unix-ms time, relative to `now`. */
export function ago(then: number, now: number): string {
  const s = Math.max(0, Math.round((now - then) / 1000))
  if (s < 60) return 'just now'
  const units: [number, string][] = [
    [86_400, 'day'],
    [3600, 'hour'],
    [60, 'minute']
  ]
  for (const [size, name] of units) {
    if (s >= size) {
      const n = Math.floor(s / size)
      return `${n} ${name}${n === 1 ? '' : 's'} ago`
    }
  }
  return 'just now'
}
