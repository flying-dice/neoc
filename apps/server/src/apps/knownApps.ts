import { bundleDescriptorSchema, knownAppsSchema, type BundleDescriptor, type BundleFile } from '@agentos/sdk/bundle'
import { z } from 'zod'

/** A known app whose bundle.json loaded and parsed. */
export interface ResolvedApp {
  /** The bundle.json URL (bundle file URLs resolve against it). */
  source: URL
  descriptor: BundleDescriptor
}

const FETCH_TIMEOUT_MS = 15_000
const MAX_REDIRECTS = 5
const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]'])

/** file: and https: anywhere; plain http: only to this machine (a sha256 fetched over http proves nothing). */
function assertAllowed(url: URL): void {
  if (url.protocol === 'file:' || url.protocol === 'https:') return
  if (url.protocol === 'http:' && LOOPBACK.has(url.hostname)) return
  throw new Error(`Unsupported app URL ${url.href} (use https:, file:, or http: on localhost)`)
}

/**
 * Fetch a URL, following redirects by hand so every hop is checked against
 * `assertAllowed` and the token is attached only while the request stays on
 * the token's origin.
 */
async function fetchChecked(first: URL, token: { origin: string; value: string } | undefined): Promise<Response> {
  let url = first
  for (let hop = 0; hop <= MAX_REDIRECTS; hop++) {
    assertAllowed(url)
    if (url.protocol === 'file:') throw new Error(`${first.href} redirects to a local file`)
    const headers: Record<string, string> = {}
    if (token && url.origin === token.origin) headers.authorization = `Bearer ${token.value}`
    const response = await fetch(url, { headers, redirect: 'manual', signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) })
    const location = response.headers.get('location')
    if (response.status < 300 || response.status >= 400 || !location) return response
    url = new URL(location, url)
  }
  throw new Error(`GET ${first.href}: too many redirects`)
}

const sha256 = (bytes: Uint8Array): string => new Bun.CryptoHasher('sha256').update(bytes).digest('hex')

/** Where known apps come from. */
export interface KnownAppsSource {
  /** A known-apps.json file, or undefined for none. */
  listFile?: URL
  /** Overrides the list file's own `base` (AGENTOS_APPS_BASE): a file: or http(s): directory URL. */
  base?: URL
  /** Extra absolute bundle.json URLs (AGENTOS_APPS). */
  extra?: string[]
  /** Bearer token for requests to the base URL's origin, and only that origin (AGENTOS_APPS_TOKEN). */
  token?: string
}

const asDirectory = (url: URL): URL => (url.href.endsWith('/') ? url : new URL(`${url.href}/`))

/**
 * The apps this server can install: the bundle.json entries in its known-apps
 * file, plus any extra URLs from config. There is no discovery: an app is
 * installable only if it's listed.
 *
 * Entries are absolute URLs or relative to the list's `base` (the one value to
 * change when bundles move to a CDN; `AGENTOS_APPS_BASE` overrides it), and
 * with no base relative to the list file itself. Everything is re-read on each
 * call, so a rebuilt bundle or an edited list shows up at once.
 */
export class KnownApps {
  constructor(private readonly options: KnownAppsSource) {}

  /** The directory entries resolve against, if any (the override, else the list's `base`). */
  async base(): Promise<URL | undefined> {
    if (this.options.base) return asDirectory(this.options.base)
    const list = await this.#readList()
    return list?.base ? asDirectory(new URL(list.base, this.options.listFile)) : undefined
  }

  async #readList(): Promise<{ base?: string; apps: string[] } | undefined> {
    const { listFile } = this.options
    if (!listFile) return undefined
    const file = Bun.file(listFile)
    if (!(await file.exists())) {
      console.warn(`[agentos] no known-apps file at ${listFile.pathname}`)
      return undefined
    }
    const parsed = knownAppsSchema.safeParse(await file.json())
    if (!parsed.success) throw new Error(`${listFile.pathname}: ${z.prettifyError(parsed.error)}`)
    return parsed.data
  }

  /** The bundle.json URLs, resolved. An entry that doesn't resolve is reported and skipped. */
  async sources(): Promise<URL[]> {
    const urls: URL[] = []
    const add = (entry: string, against: URL | string | undefined): void => {
      try {
        urls.push(new URL(entry, against))
      } catch {
        console.warn(`[agentos] known app "${entry}" skipped: not a valid URL`)
      }
    }
    const list = await this.#readList()
    if (list) {
      const base = await this.base()
      for (const entry of list.apps) add(entry, base ?? this.options.listFile)
    }
    for (const entry of this.options.extra ?? []) add(entry, undefined)
    return urls
  }

  async #read(url: URL): Promise<Uint8Array> {
    assertAllowed(url)
    if (url.protocol === 'file:') {
      const file = Bun.file(url)
      if (!(await file.exists())) throw new Error(`${url.pathname} not found`)
      return new Uint8Array(await file.arrayBuffer())
    }
    // The token is for the base URL's origin only: never another host.
    const base = await this.base()
    const token = this.options.token && base && base.origin !== 'null' ? { origin: base.origin, value: this.options.token } : undefined
    const response = await fetchChecked(url, token)
    if (!response.ok) throw new Error(`GET ${url.href} → ${response.status}`)
    return new Uint8Array(await response.arrayBuffer())
  }

  /** Load every known app; a broken entry is reported and skipped, not fatal. First listing of an id wins. */
  async load(): Promise<ResolvedApp[]> {
    const results = await Promise.all(
      (await this.sources()).map(async (source): Promise<ResolvedApp | undefined> => {
        try {
          const parsed = bundleDescriptorSchema.safeParse(JSON.parse(new TextDecoder().decode(await this.#read(source))))
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
    const bytes = await this.#read(url)
    const actual = sha256(bytes)
    if (actual !== file.sha256) throw new Error(`Integrity check failed for ${url.href}: expected ${file.sha256}, got ${actual}`)
    return bytes
  }
}
