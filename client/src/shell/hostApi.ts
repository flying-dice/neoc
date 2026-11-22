/**
 * What the shell asks the host: the app lifecycle (`app.*`), the catalog Discover
 * browses, the paired devices (`host.devices.*`) and the files apps keep (`host.files.*`). The shapes are generated from
 * `crates/vos-domain/src/api.rs` into `types/generated/vos.ts`.
 */
import { icons, type IconName } from '@vos/ui'
import type { CallOptions } from './channel'
import type {
  HostAppNotification,
  HostAppPreview,
  HostAppState,
  HostAppSummary,
  HostCatalog,
  HostDeviceRow,
  HostFileEntry,
  HostFileLocations
} from './protocol'

// The wire shapes are the generated ones (the Rust schema in `crates/vos-domain/src/api.rs`, re-exported by
// `protocol.ts`, the shell's one door to `types/generated`) under the names the shell uses, so a wire
// change is a compile error there and here, and nowhere else.
export type AppState = HostAppState
export type AppSummary = HostAppSummary
export type AppPreview = HostAppPreview
export type Catalog = HostCatalog
export type DeviceRow = HostDeviceRow
/** One entry of a folder (`host.files.list`): a name, never a path on the host. */
export type FileEntry = HostFileEntry
/** What the Files page can browse besides the apps, and how big an upload may be (`host.files.locations`). */
export type FileLocations = HostFileLocations
/** What an app's backend asked to tell the person (`notification.deliver`): the sender's id, which the host sets, a title and a body. */
export type AppNotification = HostAppNotification

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

/**
 * A place the Files page browses: This Computer (the host's own file system, all of it, by absolute path), or the
 * Documents of an installed app (by a path inside them).
 */
export type FilePlace = { kind: 'computer' } | { kind: 'app'; app: AppSummary }

/** What `host.files.*` takes to name a place: an app's id, or nothing for This Computer. */
export const placeParams = (place: FilePlace): { app?: string } =>
  place.kind === 'app' ? { app: place.app.id } : {}

/**
 * Where a file of a place is downloaded from (`GET`: always an attachment, never a page) and uploaded to (`PUT`):
 * `/files/host/{path}` or `/files/app/{app}/{path}`.
 */
export function fileUrl(place: FilePlace, path: string[]): string {
  const root = place.kind === 'app' ? `app/${encodeURIComponent(place.app.id)}` : 'host'
  return `/files/${root}/${path.map(encodeURIComponent).join('/')}`
}

/** A size for people: `512 bytes`, `1.5 KB`, `20 MB` (powers of 1000, like the Files app of the tablet it looks like). */
export function fileSize(bytes: number): string {
  if (bytes < 1000) return bytes === 1 ? '1 byte' : `${bytes} bytes`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes / 1000
  let unit = 0
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000
    unit++
  }
  return `${value < 10 ? Math.round(value * 10) / 10 : Math.round(value)} ${units[unit]}`
}
