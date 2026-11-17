import type { IconName } from '@agentos/ui'

/**
 * Static description of an app. Imported by both host processes, so it must
 * stay plain data — no Node or DOM APIs.
 */
export interface AppManifest {
  /** Stable identifier: lowercase, digits and dashes (used to namespace IPC and tools). */
  id: string
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
  return manifest
}
