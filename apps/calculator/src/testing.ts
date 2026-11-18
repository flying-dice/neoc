/**
 * Test helpers for the Calculator: a memory host running the real server
 * module, and the scoped API its client gets. Used by stories.
 */
import { appApiFor, createMemoryHost, type MemoryHost } from '@agentos/sdk/testing'
import type { AppClientApi } from '@agentos/sdk/app/client'
import manifest from './manifest'
import server from './server'

export function calculatorHost(storage?: Record<string, unknown>): MemoryHost {
  return createMemoryHost({ apps: [{ manifest, server, storage }] })
}

/** API for one Calculator window; `seed` runs after the window's runtime exists (so emitted events queue). */
export function calculatorApi(seed?: (host: MemoryHost) => void): AppClientApi {
  const host = calculatorHost()
  const api = appApiFor(host, manifest.id)
  seed?.(host)
  return api
}

export { manifest }
