/**
 * Every app module in this repo, run from source, for web mode and Storybook
 * (the real server runs their built bundles instead). Discovered by path, so a
 * new app under apps/<dir>/ shows up with no wiring:
 *
 *   src/manifest.ts                  required
 *   src/mock/server.ts               used in the browser if present (server modules that
 *                                    need bun:sqlite or other Bun APIs ship one)
 *   src/server/index.ts              otherwise, run in the page on the in-memory host
 *   src/client/index.ts              loaded lazily when the app's window opens
 */
import type { AppManifest } from '@agentos/sdk'
import type { AppClientModule } from '@agentos/sdk/app/client'
import type { AppServerModule } from '@agentos/sdk/app/server'
import type { MemoryApp } from '@agentos/sdk/testing'
import { registerSourceLoader } from '../lib/appLoaders'

const manifests = import.meta.glob<{ default: AppManifest }>('../../../../../apps/*/src/manifest.ts', { eager: true })
const mocks = import.meta.glob<{ default: AppServerModule }>('../../../../../apps/*/src/mock/server.ts', { eager: true })
const servers = import.meta.glob<{ default: AppServerModule }>('../../../../../apps/*/src/server/index.ts', { eager: true })
const clients = import.meta.glob<{ default: AppClientModule }>('../../../../../apps/*/src/client/index.ts')

const dirOf = (manifestPath: string): string => manifestPath.slice(0, -'/src/manifest.ts'.length)

/** Repo apps for `createMemoryHost({ apps })`; registers their client source loaders. */
export function repoApps(): MemoryApp[] {
  return Object.entries(manifests).map(([path, { default: manifest }]) => {
    const dir = dirOf(path)
    const client = clients[`${dir}/src/client/index.ts`]
    if (client) registerSourceLoader(manifest.id, client)
    return { manifest, server: (mocks[`${dir}/src/mock/server.ts`] ?? servers[`${dir}/src/server/index.ts`])?.default }
  })
}
