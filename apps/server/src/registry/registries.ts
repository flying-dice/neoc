import { registrySchema, type Registry, type RegistryEntry } from '@agentos/sdk/registry'
import { z } from 'zod'

export interface LoadedRegistry {
  /** Where registry.json came from (bundle URLs resolve against it). */
  url: URL
  registry: Registry
}

async function readUrl(url: URL): Promise<Uint8Array> {
  if (url.protocol === 'file:') {
    const file = Bun.file(url)
    if (!(await file.exists())) throw new Error(`${url.pathname} not found`)
    return new Uint8Array(await file.arrayBuffer())
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') throw new Error(`Unsupported registry URL ${url}`)
  const response = await fetch(url)
  if (!response.ok) throw new Error(`GET ${url} → ${response.status}`)
  return new Uint8Array(await response.arrayBuffer())
}

const sha256 = (bytes: Uint8Array): string => new Bun.CryptoHasher('sha256').update(bytes).digest('hex')

/**
 * The registries this server installs from. A registry is any URL (http(s): or
 * file:) of a registry.json; this repo's own is the first. Registries are
 * re-read on every listing so a rebuilt or updated registry shows up at once.
 */
export class Registries {
  constructor(private readonly sources: URL[]) {}

  /** Read every registry; a broken one is reported and skipped, not fatal. */
  async load(): Promise<LoadedRegistry[]> {
    const loaded: LoadedRegistry[] = []
    for (const url of this.sources) {
      try {
        const json: unknown = JSON.parse(new TextDecoder().decode(await readUrl(url)))
        const parsed = registrySchema.safeParse(json)
        if (!parsed.success) throw new Error(z.prettifyError(parsed.error))
        if (loaded.some((r) => r.registry.name === parsed.data.name)) {
          throw new Error(`duplicate registry name "${parsed.data.name}"`)
        }
        loaded.push({ url, registry: parsed.data })
      } catch (error) {
        console.warn(`[agentos] registry ${url} skipped:`, error instanceof Error ? error.message : error)
      }
    }
    return loaded
  }

  async find(registryName: string, appId: string): Promise<{ source: LoadedRegistry; entry: RegistryEntry }> {
    const source = (await this.load()).find((r) => r.registry.name === registryName)
    if (!source) throw new Error(`Unknown registry "${registryName}"`)
    const entry = source.registry.apps.find((a) => a.manifest.id === appId)
    if (!entry) throw new Error(`Registry "${registryName}" has no app "${appId}"`)
    return { source, entry }
  }

  /** Download a bundle and verify it against the registry's sha256 before anything uses it. */
  async download(source: LoadedRegistry, ref: RegistryEntry['server']): Promise<Uint8Array> {
    const url = new URL(ref.url, source.url)
    const bytes = await readUrl(url)
    const actual = sha256(bytes)
    if (actual !== ref.sha256) throw new Error(`Integrity check failed for ${url}: expected ${ref.sha256}, got ${actual}`)
    return bytes
  }
}
