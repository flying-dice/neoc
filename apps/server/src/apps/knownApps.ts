import { bundleDescriptorSchema, knownAppsSchema, type BundleDescriptor, type BundleFile } from '@agentos/sdk/bundle'
import { z } from 'zod'

/** A known app whose bundle.json loaded and parsed. */
export interface ResolvedApp {
  /** The bundle.json URL (bundle file URLs resolve against it). */
  source: URL
  descriptor: BundleDescriptor
}

const FETCH_TIMEOUT_MS = 15_000
const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]'])

/** file: and https: anywhere; plain http: only to this machine (a sha256 fetched over http proves nothing). */
function assertAllowed(url: URL): void {
  if (url.protocol === 'file:' || url.protocol === 'https:') return
  if (url.protocol === 'http:' && LOOPBACK.has(url.hostname)) return
  throw new Error(`Unsupported app URL ${url.href} (use https:, file:, or http: on localhost)`)
}

async function read(url: URL): Promise<Uint8Array> {
  assertAllowed(url)
  if (url.protocol === 'file:') {
    const file = Bun.file(url)
    if (!(await file.exists())) throw new Error(`${url.pathname} not found`)
    return new Uint8Array(await file.arrayBuffer())
  }
  const response = await fetch(url, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) })
  if (!response.ok) throw new Error(`GET ${url.href} → ${response.status}`)
  return new Uint8Array(await response.arrayBuffer())
}

const readJson = async (url: URL): Promise<unknown> => JSON.parse(new TextDecoder().decode(await read(url)))
const sha256 = (bytes: Uint8Array): string => new Bun.CryptoHasher('sha256').update(bytes).digest('hex')

/**
 * The apps this server can install: the bundle.json URLs in its known-apps
 * file (relative entries resolve against the file), plus any extra URLs from
 * config. There is no discovery: an app is installable only if it's listed.
 * Everything is re-read on each call, so a rebuilt bundle or an edited list
 * shows up at once.
 */
export class KnownApps {
  constructor(
    /** A known-apps.json file, or undefined for none. */
    private readonly listFile: URL | undefined,
    private readonly extra: string[] = []
  ) {}

  async sources(): Promise<URL[]> {
    const urls = this.extra.map((u) => new URL(u))
    if (!this.listFile) return urls
    const file = Bun.file(this.listFile)
    if (!(await file.exists())) {
      console.warn(`[agentos] no known-apps file at ${this.listFile.pathname}`)
      return urls
    }
    const parsed = knownAppsSchema.safeParse(await file.json())
    if (!parsed.success) throw new Error(`${this.listFile.pathname}: ${z.prettifyError(parsed.error)}`)
    return [...parsed.data.apps.map((entry) => new URL(entry, this.listFile)), ...urls]
  }

  /** Load every known app; a broken entry is reported and skipped, not fatal. First listing of an id wins. */
  async load(): Promise<ResolvedApp[]> {
    const results = await Promise.all(
      (await this.sources()).map(async (source): Promise<ResolvedApp | undefined> => {
        try {
          const parsed = bundleDescriptorSchema.safeParse(await readJson(source))
          if (!parsed.success) throw new Error(z.prettifyError(parsed.error))
          return { source, descriptor: parsed.data }
        } catch (error) {
          console.warn(`[agentos] known app ${source.href} skipped:`, error instanceof Error ? error.message : error)
          return undefined
        }
      })
    )
    const byId = new Map<string, ResolvedApp>()
    for (const app of results) {
      if (!app) continue
      const id = app.descriptor.manifest.id
      if (byId.has(id)) console.warn(`[agentos] known app ${app.source.href} skipped: "${id}" is already listed`)
      else byId.set(id, app)
    }
    return [...byId.values()]
  }

  async find(appId: string): Promise<ResolvedApp> {
    const app = (await this.load()).find((a) => a.descriptor.manifest.id === appId)
    if (!app) throw new Error(`Unknown app "${appId}"`)
    return app
  }

  /** Fetch a bundle file and check its sha256 before anything uses it. */
  async download(app: ResolvedApp, file: BundleFile): Promise<Uint8Array> {
    const url = new URL(file.url, app.source)
    // A hosted descriptor can't point the server at its own disk.
    if (url.protocol === 'file:' && app.source.protocol !== 'file:') throw new Error(`${app.source.href} points at a local file`)
    const bytes = await read(url)
    const actual = sha256(bytes)
    if (actual !== file.sha256) throw new Error(`Integrity check failed for ${url.href}: expected ${file.sha256}, got ${actual}`)
    return bytes
  }
}
