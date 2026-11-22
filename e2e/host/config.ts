/**
 * Where the end-to-end host lives. One host serves every spec: a fresh data directory under test-results/,
 * the built client, a test registry, a port that is not the default (7400, see scripts/ports.ts), so it never
 * collides with a host a person has running.
 */
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { HOST_PORT } from '../../scripts/ports'

export const ROOT = resolve(import.meta.dirname, '../..')
export const PORT = HOST_PORT
/** `127.0.0.1`, not `localhost`: the session cookie is host-only, so every URL must use the same name. */
export const ORIGIN = `http://127.0.0.1:${PORT}`
export const DIR = join(ROOT, 'test-results/host')
export const DATA = join(DIR, 'data')
export const CONFIG = join(DIR, 'vos.toml')
export const REGISTRY = join(DIR, 'registry/registry.json')
/**
 * The Ticker (background work, `e2e/background.journeys.spec.ts`): built like any app, but kept out of the registry so
 * Discover does not offer it; the spec installs it from this app ref.
 */
export const TICKER_APP_REF = `file://${join(DIR, 'hidden/ticker/0.1.0/app-ref.json')}`
/** The framework fixtures (e2e/apps): a task list with a Svelte client and one with a React client. */
export const FIXTURE_APPS = ['tasks-svelte', 'tasks-react'] as const
export type FixtureApp = (typeof FIXTURE_APPS)[number]

/** The app ref a fixture is installed from: built outside the registry (e2e/host/start.ts), so Discover never offers it. */
export const fixtureAppRef = (id: FixtureApp): string => {
  const { info } = JSON.parse(readFileSync(join(ROOT, 'e2e/apps', id, 'openrpc.json'), 'utf8')) as { info: { version: string } }
  const version = info.version
  return `file://${join(DIR, `fixtures/${id}/${version}/app-ref.json`)}`
}
/** The Notifier (`e2e/notifications.journeys.spec.ts`): a sender of notifications, kept out of the registry like the Ticker. */
export const NOTIFIER_APP_REF = `file://${join(DIR, 'hidden/notifier/0.1.0/app-ref.json')}`
/** The paired browser the specs share (a Playwright storage state). */
export const STATE = join(DIR, 'paired.json')
export const HOST_BIN = join(ROOT, 'target/debug/vos')
/** The scripted ACP agent (no model): the prompt text picks what it does. */
export const FAKE_AGENT = join(ROOT, 'target/debug/fake_agent')
