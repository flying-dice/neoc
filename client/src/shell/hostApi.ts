/**
 * What the shell asks the host besides the view protocol: the app lifecycle (`app.*`), the catalog Discover
 * browses, the agent roster (`agents.*`) and the paired devices (`host.devices.*`). The shapes are generated from
 * `crates/vos-domain/src/api.rs` into `types/generated/vos.ts`.
 */
import { icons, type IconName } from '@vos/ui'
import type { CallOptions } from './channel'
import type {
  HostAgentList,
  HostAgentRow,
  HostAppPreview,
  HostAppState,
  HostAppSummary,
  HostCatalog,
  HostDeviceRow,
  HostInstalledInfo
} from '../view/protocol'

// The wire shapes are the generated ones (the Rust schema in `crates/vos-domain/src/api.rs`, re-exported by
// `view/protocol.ts`, the renderer's one door to `types/generated`) under the names the shell uses, so a wire
// change is a compile error there and here, and nowhere else.
export type AppState = HostAppState
export type AppSummary = HostAppSummary
export type InstalledInfo = HostInstalledInfo
export type AppPreview = HostAppPreview
export type Catalog = HostCatalog
/** One agent on the host's roster (`agents.list`): no tokens, and only the names of its environment variables. */
export type AgentRow = HostAgentRow
export type AgentList = HostAgentList
/** What the shell shows of an agent (the roster row also carries its launch command and grants, which the shell never reads). */
export type AgentSummary = Pick<HostAgentRow, 'id' | 'name' | 'avatar' | 'role'>
export type DeviceRow = HostDeviceRow

/** The calls the shell makes. The real one is the session's `call`; stories and tests pass a fake. */
export interface HostApi {
  call(method: string, params?: Record<string, unknown>, options?: CallOptions): Promise<unknown>
  /** Host notifications that no window owns (`window.open`, `app.installed`, ...). */
  onNotification(listener: (method: string, params: Record<string, unknown>) => void): () => void
}

/** The icon of an app: its declared symbol when the kit has it, else a generic one. */
export function iconOf(name: string | undefined): IconName {
  return name && Object.hasOwn(icons, name) ? (name as IconName) : 'square.grid'
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
