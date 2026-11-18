import type { IconName } from '@agentos/ui'

/**
 * Static description of an app. Read by the server, the client and the bundle
 * build (it becomes the bundle's manifest.json), so it must stay plain data.
 */
export interface AppManifest {
  /** Stable identifier: lowercase, digits and dashes (namespaces RPC, tools and storage). */
  id: string
  /** Semver of the app bundle; known apps and updates compare it. */
  version: string
  name: string
  icon: IconName
  /** CSS background for the app icon, typically a gradient. */
  background: string
  /**
   * Preferred floating window size, and the smallest size the app's layout
   * supports (the window can't be resized below it; default 320x240).
   */
  window?: { width: number; height: number; minWidth?: number; minHeight?: number }
  /** Shown in the Dock when true. */
  dock?: boolean
}

export function defineManifest<const M extends AppManifest>(manifest: M): M {
  if (!/^[a-z][a-z0-9-]{1,31}$/.test(manifest.id)) {
    throw new Error(`Invalid app id "${manifest.id}": use lowercase letters, digits and dashes`)
  }
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(manifest.version)) {
    throw new Error(`Invalid version "${manifest.version}" for app "${manifest.id}": use semver (1.2.3)`)
  }
  return manifest
}
