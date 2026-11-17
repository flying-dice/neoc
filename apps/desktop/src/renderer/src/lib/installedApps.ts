/**
 * Installed app modules — renderer registry. Mirrors the main-process
 * registry; each entry lazy-loads its renderer bundle on first open.
 */
import type { AppManifest } from '@agentos/sdk'
import type { AppRendererModule } from '@agentos/sdk/app/renderer'
import calculatorManifest from '@agentos/app-calculator/manifest'

export interface InstalledRendererApp {
  manifest: AppManifest
  load: () => Promise<{ default: AppRendererModule }>
}

export const installedApps: InstalledRendererApp[] = [
  { manifest: calculatorManifest, load: () => import('@agentos/app-calculator/renderer') }
]

export const moduleApp = (id: string): InstalledRendererApp | undefined =>
  installedApps.find((a) => a.manifest.id === id)
