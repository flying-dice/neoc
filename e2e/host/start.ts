#!/usr/bin/env bun
/**
 * Starts the real host for the end-to-end specs (Playwright's webServer runs this):
 *
 *   1. builds the app kit (client/public/kit), the client (client/dist) and the host (cargo), and the apps: the repo's apps
 *      into a test registry, with Memos (e2e/apps/memos), a second app the Discover journeys install and remove so they
 *      never disturb the Calculator the other specs use; and the rest of e2e/apps into a folder no registry lists: the two
 *      framework fixtures (a task list with a Svelte client and one with a React client, each on a Luau backend),
 *      installed at start like the Calculator, and the Ticker (background work), the Notifier (the notification
 *      journeys' sender) and the Journal at two versions (the update journey's), which only their journeys install;
 *   2. writes vos.toml (port 7410 or VOS_PORT_BASE + 10, loopback, a fresh data directory, the test registry; the Calculator,
 *      Agents, Copilot, Todos, Notes, Containers, DNS, Pages and Processes are installed at start, Memos is only offered);
 *   3. runs vos-host in the foreground, forwarding SIGTERM so the host shuts down gracefully. The host's own Docker is
 *      one nothing listens at (`DOCKER_HOST`), so the Containers, DNS, Pages and Copilot apps say "not reachable" on every machine, with
 *      Docker running or not, until their journeys point them at a Docker of their own (apps/containers/e2e, apps/dns/e2e, apps/pages/e2e,
 *      apps/copilot/e2e).
 *      The Terminal app (installed at start too) runs the host's default shell (`SHELL`; on Windows `ComSpec`): here a plain
 *      `sh` (on Windows a plain PowerShell) with the prompt `$ ` that reads no start-up file, so what its window shows is the
 *      same on every machine of a platform (apps/terminal/e2e, the desktop screenshots).
 *      The keychain the host reaches is a mock one (`NEOC_KEYRING`), so the Keychain app's journeys (apps/keychain/e2e) keep their passwords in the host's
 *      memory and never in the keychain of whoever runs the specs. The service manager is a mock one too (`NEOC_SERVICE_MANAGER`): the Services
 *      app's journeys (apps/services/e2e) start, stop and remove services of the host's memory, never of the machine that runs the specs.
 */
import { spawn, spawnSync } from 'node:child_process'
import { chmodSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { CONFIG, DATA, DIR, FIXTURE_APPS, HIDDEN_APPS, HOST_BIN, INSTALLED_REPO_APPS, PORT, REGISTRY, REGISTRY_APPS, ROOT, appMeta, fileUrl, fixtureAppRef, portable } from './config'

const run = (cmd: string, args: string[]): void => {
  const r = spawnSync(cmd, args, { cwd: ROOT, stdio: 'inherit' })
  if (r.status !== 0) process.exit(r.status ?? 1)
}

/** The version a repo app ships (its package.json's `version`), so a bump needs no change here. */
const versionOf = (id: string): string => appMeta(`apps/${id}`).version

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
// Ticker, the Notifier and the Journal are installed by their journeys
run('bun', ['scripts/build-apps.ts', ...FIXTURE_APPS, ...HIDDEN_APPS, '--out', join(DIR, 'fixtures'), '--root', join(ROOT, 'e2e/apps')])

mkdirSync(DATA, { recursive: true })
// every path is written with `/` (`portable`): a `\` in a TOML basic string starts an escape
writeFileSync(
  CONFIG,
  [
    `data_dir = "${portable(DATA)}"`,
    `bind = "127.0.0.1"`,
    `port = ${PORT}`,
    `client_dir = "${portable(join(ROOT, 'client/dist'))}"`,
    `registries = ["${fileUrl(REGISTRY)}"]`,
    `install = [${[
      ...INSTALLED_REPO_APPS.map((id) => fileUrl(join(DIR, 'registry', id, versionOf(id), 'app-ref.json'))),
      ...FIXTURE_APPS.map(fixtureAppRef)
    ]
      .map((url) => `"${url}"`)
      .join(', ')}]`,
    ''
  ].join('\n')
)

/**
 * The Terminal app's shell (see the header): the person's own would print their prompt, with the machine's name in it.
 * The host starts the default shell with no arguments, so the shell is a program that starts the real one with its own:
 * on Unix `SHELL`, a script for a `sh` that reads no start-up file; on Windows `ComSpec` (what the default shell is
 * there), which has to be an executable: `shell.rs`, compiled here, which starts a plain Windows PowerShell. Both have
 * the prompt `$ `.
 */
function shellEnv(): Record<string, string> {
  if (process.platform !== 'win32') {
    const shell = join(DIR, 'shell')
    writeFileSync(shell, "#!/bin/sh\nPS1='$ ' ENV= exec /bin/sh\n")
    chmodSync(shell, 0o755)
    return { SHELL: shell }
  }
  const shell = join(DIR, 'shell.exe')
  run('rustc', ['--edition', '2021', '-O', '-o', shell, join(import.meta.dirname, 'shell.rs')])
  return { ComSpec: shell }
}

// On Windows a variable's name is in whatever case it was set with (`ComSpec`, `COMSPEC`): the one given here replaces it
const shell = shellEnv()
const given = new Set(Object.keys(shell).map((name) => name.toLowerCase()))
const inherited = Object.entries(process.env).filter(([name]) => process.platform !== 'win32' || !given.has(name.toLowerCase()))
// A Docker nothing listens at, which says so at once: a port of this machine nothing has (a connection is refused); on
// Windows a pipe that is not there (a refused connection is tried again there for seconds before it is given up)
const DOCKER_HOST = process.platform === 'win32' ? 'npipe:////./pipe/vos-e2e-no-docker' : 'tcp://127.0.0.1:1'
const env = { ...Object.fromEntries(inherited), DOCKER_HOST, NEOC_KEYRING: 'mock', NEOC_SERVICE_MANAGER: 'mock', ...shell }

const host = spawn(HOST_BIN, ['--config', CONFIG], { cwd: ROOT, stdio: 'inherit', env })
for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, () => host.kill('SIGTERM'))
host.on('exit', (code) => process.exit(code ?? 0))
