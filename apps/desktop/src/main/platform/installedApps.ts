/**
 * Installed app modules — main-process registry.
 *
 * Every app lives in its own workspace package under `apps/<id>` and is
 * registered here (and in the renderer's `lib/installedApps.ts`). The host
 * only ever talks to apps through the SDK contract.
 */
import type { AppManifest } from '@agentos/sdk'
import type { AppMainModule } from '@agentos/sdk/app/main'
import calculatorManifest from '@agentos/app-calculator/manifest'
import calculatorMain from '@agentos/app-calculator/main'

export interface InstalledApp {
  manifest: AppManifest
  main: AppMainModule
}

export const installedApps: InstalledApp[] = [
  { manifest: calculatorManifest, main: calculatorMain }
]
