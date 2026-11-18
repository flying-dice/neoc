import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

// Not 7000: macOS's AirPlay Receiver (ControlCenter) listens on it by default.
const DEFAULT_PORT = 7400
const repoRoot = resolve(import.meta.dir, '../../..')

export interface ServerConfig {
  /** Interface to bind. Default 127.0.0.1: AgentOS has no user auth yet, so don't expose it. */
  hostname: string
  port: number
  /** Installed bundles, per-app data (SQLite etc.) and the system database. */
  dataDir: string
  /** The built PWA client to serve (vite build output). */
  clientDir: string
  /** The known-apps file: the bundle.json URLs this server offers (default: the built-in list). */
  knownAppsFile: string
  /** Overrides the known-apps list's `base` (AGENTOS_APPS_BASE): a directory URL (https:, file:, http: on localhost) or path. */
  appsBase: URL | undefined
  /** Bearer token for the apps base's origin only (AGENTOS_APPS_TOKEN), for a private repo. */
  appsToken: string | undefined
  /** Extra absolute bundle.json URLs to offer (https:, file:, or http: on localhost). */
  extraApps: string[]
  /** Browser origins allowed to open /rpc besides the server's own (e.g. the Vite dev server). */
  allowedOrigins: string[]
  /** Dev mode: reload installed apps when their bundles are rebuilt. */
  dev: boolean
}

const list = (v: string | undefined): string[] =>
  (v ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)

/** A URL as given, or a filesystem path turned into a file: URL (relative paths resolve against the cwd). */
function directoryUrl(value: string | undefined): URL | undefined {
  if (!value) return undefined
  const url = /^[a-z][a-z0-9+.-]+:/i.test(value) ? new URL(value) : pathToFileURL(resolve(value))
  return url.href.endsWith('/') ? url : new URL(`${url.href}/`)
}

export function loadConfig(env: Record<string, string | undefined> = process.env): ServerConfig {
  const port = Number(env.AGENTOS_PORT ?? DEFAULT_PORT)
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error(`Invalid AGENTOS_PORT "${env.AGENTOS_PORT}"`)
  return {
    hostname: env.AGENTOS_HOST ?? '127.0.0.1',
    port,
    dataDir: resolve(env.AGENTOS_DATA ?? join(homedir(), '.agentos')),
    clientDir: resolve(env.AGENTOS_CLIENT_DIR ?? join(repoRoot, 'apps/desktop/dist')),
    knownAppsFile: resolve(env.AGENTOS_KNOWN_APPS ?? join(repoRoot, 'apps/server/known-apps.json')),
    appsBase: directoryUrl(env.AGENTOS_APPS_BASE),
    appsToken: env.AGENTOS_APPS_TOKEN || undefined,
    extraApps: list(env.AGENTOS_APPS),
    allowedOrigins: list(env.AGENTOS_ALLOWED_ORIGINS),
    dev: env.AGENTOS_DEV === '1'
  }
}
