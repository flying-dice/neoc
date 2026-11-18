import { homedir } from 'node:os'
import { join, resolve } from 'node:path'

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
  /** Extra bundle.json URLs to offer (https:, file:, or http: on localhost). */
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

export function loadConfig(env: Record<string, string | undefined> = process.env): ServerConfig {
  const port = Number(env.AGENTOS_PORT ?? 7000)
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error(`Invalid AGENTOS_PORT "${env.AGENTOS_PORT}"`)
  return {
    hostname: env.AGENTOS_HOST ?? '127.0.0.1',
    port,
    dataDir: resolve(env.AGENTOS_DATA ?? join(homedir(), '.agentos')),
    clientDir: resolve(env.AGENTOS_CLIENT_DIR ?? join(repoRoot, 'apps/desktop/dist')),
    knownAppsFile: resolve(env.AGENTOS_KNOWN_APPS ?? join(repoRoot, 'apps/server/known-apps.json')),
    extraApps: list(env.AGENTOS_APPS),
    allowedOrigins: list(env.AGENTOS_ALLOWED_ORIGINS),
    dev: env.AGENTOS_DEV === '1'
  }
}
