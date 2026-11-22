/**
 * Wire types of what the shell says to the host and hears from it.
 *
 * The Rust schema is the source of truth: `types/generated/vos.ts` is generated from it (`bun run codegen`).
 * This module is the shell's only door to it: it re-exports the generated shapes the shell uses, under the names
 * the shell uses, so a wire change is a compile error here and nowhere else. Nothing else in the client declares a
 * wire message or imports the generated module directly.
 */
import type * as G from '../../../types/generated/vos'

export { PROTO_VERSION as PROTO, KIT_VERSION as KIT } from '../../../types/generated/vos'

// ── The handshake ───────────────────────────────────────────────

export type SessionHello = G.SessionHello
export type SessionWelcome = G.SessionWelcome

// ── Host management: the shell's own calls (`app.*`, `host.devices.*`, `host.files.*`) ─────────────────────────

export type HostAppState = G.AppState
export type HostAppSummary = G.AppSummary
export type HostAppEvent = G.AppEvent
export type HostAppPreview = G.AppPreview
export type HostCatalog = G.Catalog
export type HostDeviceRow = G.DeviceRow
export type HostFileEntry = G.FileEntry
export type HostFileLocations = G.FileLocations

// ── Host -> client notifications ────────────────────────────────

/** What an app's backend asked to tell the person (`notification.deliver`). */
export type HostAppNotification = G.AppNotification
