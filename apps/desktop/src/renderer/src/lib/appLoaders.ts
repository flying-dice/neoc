import type { AppClientModule } from '@agentos/sdk/app/client'
import type { InstalledApp } from '@agentos/sdk/host/client'

type Loader = () => Promise<{ default: AppClientModule }>

/**
 * Where an app's client code comes from. Normally the installed bundle at
 * `clientUrl` (served by the server). Web mode and Storybook register source
 * loaders instead, so the shell can run apps straight from the repo.
 */
const sourceLoaders = new Map<string, Loader>()

export function registerSourceLoader(appId: string, loader: Loader): void {
  sourceLoaders.set(appId, loader)
}

const isClientModule = (value: unknown): value is AppClientModule =>
  typeof value === 'object' &&
  value !== null &&
  typeof (value as AppClientModule).component === 'function'

export async function loadClientApp(app: InstalledApp): Promise<AppClientModule> {
  const source = sourceLoaders.get(app.manifest.id)
  const mod = (await (source ? source() : import(/* @vite-ignore */ app.clientUrl))) as {
    default?: unknown
  }
  if (!isClientModule(mod.default)) {
    throw new Error(
      `${app.manifest.name}'s client bundle must default-export defineClientApp({ component })`
    )
  }
  return mod.default
}
