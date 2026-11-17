/**
 * Test helpers for the Calculator: a memory host running the real main
 * module, and the scoped API its renderer gets. Used by stories.
 */
import { appApiFor, createMemoryHost, type MemoryHost } from '@agentos/sdk/testing'
import type { AppRendererApi } from '@agentos/sdk/app/renderer'
import manifest from './manifest'
import main from './main'

export function calculatorHost(storage?: Record<string, unknown>): MemoryHost {
  return createMemoryHost({ apps: [{ manifest, main, storage }] })
}

/** API for one Calculator window; `seed` runs after the window's runtime exists (so emitted events queue). */
export function calculatorApi(seed?: (host: MemoryHost) => void): AppRendererApi {
  const host = calculatorHost()
  const api = appApiFor(host, manifest.id)
  seed?.(host)
  return api
}

export { manifest }
