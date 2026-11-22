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
/**
 * The host's own folder. With `VOS_PORT_BASE` set (a second run beside another, on ports of its own: scripts/ports.ts) it
 * is a folder of its own too, so one run's start never removes the data directory under the other's host.
 */
export const DIR = join(ROOT, process.env.VOS_PORT_BASE ? `test-results/host-${process.env.VOS_PORT_BASE}` : 'test-results/host')
export const DATA = join(DIR, 'data')
export const CONFIG = join(DIR, 'vos.toml')
/**
 * Where the Files journeys make the directories they browse on "This Computer" (the host could go anywhere on the disk; the
 * journeys go only here, by path): inside the run's own folder, which the next run removes.
 */
export const FILES_DIR = join(DIR, 'files')
export const REGISTRY = join(DIR, 'registry/registry.json')
/** The framework fixtures (e2e/apps): a task list with a Svelte client and one with a React client, installed at start. */
export const FIXTURE_APPS = ['tasks-svelte', 'tasks-react'] as const
export type FixtureApp = (typeof FIXTURE_APPS)[number]
/**
 * The apps of e2e/apps that are built like the fixtures but not installed at start, and kept out of the registry so Discover
 * does not offer them: the Ticker (background work, `e2e/background.journeys.spec.ts`) and the Notifier (a sender of
 * notifications, `e2e/notifications.journeys.spec.ts`). Their specs install them from their app refs.
 */
export const HIDDEN_APPS = ['ticker', 'notifier'] as const
/** The app of e2e/apps the test registry offers and nothing installs at start: the Discover journeys install and remove it. */
export const REGISTRY_APPS = ['memos'] as const

/** The app ref of an app of e2e/apps, built under `folder` of the host's directory (e2e/host/start.ts) at its contract's version. */
const appRef = (folder: 'fixtures' | 'registry', id: string): string => {
  const { info } = JSON.parse(readFileSync(join(ROOT, 'e2e/apps', id, 'openrpc.json'), 'utf8')) as { info: { version: string } }
  return `file://${join(DIR, folder, id, info.version, 'app-ref.json')}`
}

/** The app ref a fixture is installed from: built outside the registry (e2e/host/start.ts), so Discover never offers it. */
export const fixtureAppRef = (id: FixtureApp): string => appRef('fixtures', id)
export const TICKER_APP_REF = appRef('fixtures', 'ticker')
export const NOTIFIER_APP_REF = appRef('fixtures', 'notifier')
/** Memos, as the test registry lists it: what the Discover journeys paste to add an app by URL. */
export const MEMOS_APP_REF = appRef('registry', 'memos')
/** The paired browser the specs share (a Playwright storage state). */
export const STATE = join(DIR, 'paired.json')
export const HOST_BIN = join(ROOT, 'target/debug/vos')
