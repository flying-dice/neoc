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
  /** This repo's registry (bun run build:apps); served at /registry and installed from. */
  localRegistryDir: string
  /** Extra registries: URLs of registry.json files (http(s): or file:). */
  registries: string[]
  /** Browser origins allowed to open /rpc besides the server's own (e.g. the Vite dev server). */
  allowedOrigins: string[]
  /** Dev mode: reload apps when the local registry is rebuilt. */
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
    localRegistryDir: resolve(env.AGENTOS_REGISTRY_DIR ?? join(repoRoot, 'dist/registry')),
    registries: list(env.AGENTOS_REGISTRIES),
    allowedOrigins: list(env.AGENTOS_ALLOWED_ORIGINS),
    dev: env.AGENTOS_DEV === '1'
  }
}
