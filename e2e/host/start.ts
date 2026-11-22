#!/usr/bin/env bun
/**
 * Starts the real host for the end-to-end specs (Playwright's webServer runs this):
 *
 *   1. builds the app kit (client/public/kit), the client (client/dist) and the host (cargo), and the apps: the repo's apps
 *      into a test registry, with Memos (e2e/apps/memos), a second app the Discover journeys install and remove so they
 *      never disturb the Calculator the other specs use; and the rest of e2e/apps into a folder no registry lists: the two
 *      framework fixtures (a task list with a Svelte client and one with a React client, each on a Luau backend),
 *      installed at start like the Calculator, and the Ticker (background work) and the Notifier (the notification
 *      journeys' sender), which only their journeys install;
 *   2. writes vos.toml (port 7410 or VOS_PORT_BASE + 10, loopback, a fresh data directory, the test registry; the Calculator,
 *      Agents, Todos, Notes, Containers, DNS and Pages are installed at start, Memos is only offered);
 *   3. runs vos-host in the foreground, forwarding SIGTERM so the host shuts down gracefully. The host's own Docker is
 *      one nothing listens at (`DOCKER_HOST`), so the Containers, DNS and Pages apps say "not reachable" on every machine, with
 *      Docker running or not, until their journeys point them at a Docker of their own (apps/containers/e2e, apps/dns/e2e, apps/pages/e2e).
 *      The Terminal app (installed at start too) runs the host's `SHELL`: here a plain `sh` with the prompt `$ ` that reads no
 *      start-up file, so what its window shows is the same on every machine (apps/terminal/e2e, the desktop screenshots).
 *      The keychain the host reaches is a mock one (`NEOC_KEYRING`), so the Keychain app's journeys (apps/keychain/e2e) keep their passwords in the host's
 *      memory and never in the keychain of whoever runs the specs.
 */
import { spawn, spawnSync } from 'node:child_process'
import { chmodSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { CONFIG, DATA, DIR, FIXTURE_APPS, HIDDEN_APPS, HOST_BIN, PORT, REGISTRY, REGISTRY_APPS, ROOT, fixtureAppRef } from './config'

const run = (cmd: string, args: string[]): void => {
  const r = spawnSync(cmd, args, { cwd: ROOT, stdio: 'inherit' })
  if (r.status !== 0) process.exit(r.status ?? 1)
}

/** The version a repo app ships (its app.json, or for a Vite project its contract's `info.version`), so a bump needs no change here. */
const versionOf = (id: string): string =>
  existsSync(join(ROOT, 'apps', id, 'openrpc.json'))
    ? (JSON.parse(readFileSync(join(ROOT, 'apps', id, 'openrpc.json'), 'utf8')) as { info: { version: string } }).info.version
    : (JSON.parse(readFileSync(join(ROOT, 'apps', id, 'app.json'), 'utf8')) as { version: string }).version

rmSync(DIR, { recursive: true, force: true })
mkdirSync(DIR, { recursive: true })

run('bun', ['run', '--filter', '@vos/kit', 'build']) // /kit/kit.js and /kit/tokens.css: client/public/kit, which the client build copies
run('bun', ['run', '--filter', '@vos/client', 'build'])
run('cargo', ['build', '-q', '-p', 'vos-host'])

// The test registry: the repo's apps, and Memos (e2e/apps/memos), which the registry offers and nothing installs at start
const repoApps = readdirSync(join(ROOT, 'apps'))
run('bun', ['scripts/build-apps.ts', ...repoApps, ...REGISTRY_APPS, '--out', join(DIR, 'registry'), '--root', join(ROOT, 'e2e/apps')])

// The other apps of e2e/apps: each is built with its own package (its client is a Vite build), into a folder of its own
// that no registry lists, so Discover does not offer them. The framework fixtures are installed at start (below); the
// Ticker and the Notifier are installed by their journeys
run('bun', ['scripts/build-apps.ts', ...FIXTURE_APPS, ...HIDDEN_APPS, '--out', join(DIR, 'fixtures'), '--root', join(ROOT, 'e2e/apps')])

mkdirSync(DATA, { recursive: true })
writeFileSync(
  CONFIG,
  [
    `data_dir = "${DATA}"`,
    `bind = "127.0.0.1"`,
    `port = ${PORT}`,
    `client_dir = "${join(ROOT, 'client/dist')}"`,
    `registries = ["file://${REGISTRY}"]`,
    `install = [${[
      ...['calculator', 'agents', 'todos', 'notes', 'notebooks', 'containers', 'dns', 'keychain', 'pages', 'projects', 'terminal'].map((id) => `file://${join(DIR, `registry/${id}/${versionOf(id)}/app-ref.json`)}`),
      ...FIXTURE_APPS.map(fixtureAppRef)
    ]
      .map((url) => `"${url}"`)
      .join(', ')}]`,
    ''
  ].join('\n')
)

// the Terminal app's shell (see the header): the person's own would print their prompt, with the machine's name in it
const SHELL = join(DIR, 'shell')
writeFileSync(SHELL, "#!/bin/sh\nPS1='$ ' ENV= exec /bin/sh\n")
chmodSync(SHELL, 0o755)

const host = spawn(HOST_BIN, ['--config', CONFIG], { cwd: ROOT, stdio: 'inherit', env: { ...process.env, DOCKER_HOST: 'tcp://127.0.0.1:1', NEOC_KEYRING: 'mock', SHELL } })
for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, () => host.kill('SIGTERM'))
host.on('exit', (code) => process.exit(code ?? 0))
