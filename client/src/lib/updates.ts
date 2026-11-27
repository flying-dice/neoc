/**
 * App updates in Discover: which installed app has a newer version in the catalog, and what the person is shown before
 * it is taken. A new version is new code, and it may ask for more than the one that was installed (a browser feature
 * for its window, an origin its page loads from), so the update is approved as an install is, before anything changes.
 */
import type { AppPreview, AppSummary, Catalog } from '../shell/hostApi'

/** What an installed app can do, said before one is installed or updated: an app is the person's own code on their own host. */
export const ACCESS_NOTE =
  'Apps can read and write files, use the network and run processes on this machine.'

/** The newer version the catalog offers of the installed app `id` (the host marks it `update`), if it offers one. */
export const updateFor = (catalog: Catalog, id: string): AppPreview | undefined =>
  catalog.apps.find((a) => a.id === id && a.update)

/** `0.4.0 → 0.5.0` */
export const versionChange = (from: string, to: string): string => `${from} → ${to}`

/** The approval of an update: what changes, what the new version asks for, and what any app can do. */
export function updateApproval(
  installed: AppSummary,
  offered: AppPreview
): { title: string; message: string } {
  const lines = [`${versionChange(installed.version, offered.version)}.`]
  if (offered.permissions.length > 0) {
    const features = offered.permissions.map((p) =>
      installed.permissions.includes(p) ? p : `${p} (new)`
    )
    lines.push(`Its window may use: ${features.join(', ')}.`)
  }
  if (offered.origins.length > 0)
    lines.push(`Its page may load from: ${offered.origins.join(', ')}.`)
  lines.push(ACCESS_NOTE)
  return { title: `Update ${installed.name}?`, message: lines.join(' ') }
}
