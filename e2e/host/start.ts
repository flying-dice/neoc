#!/usr/bin/env bun
/**
 * Starts the real host for the end-to-end specs (Playwright's webServer runs this):
 *
 *   1. builds the client (client/dist) and the host (cargo), and the apps into a test registry:
 *      the repo's apps plus Notes (the codegen fixture), a second app the Discover journeys install and remove
 *      so they never disturb the Calculator the other specs use, and the Ticker (the host's background-work
 *      fixture) and the Notifier (the notification journeys' sender) outside the registry, which only their journeys install; and the two framework fixtures
 *      (e2e/apps: a task list with a Svelte client and one with a React client, each on a Luau backend), built
 *      into a folder no registry lists and installed at start like the Calculator;
 *   2. writes vos.toml (port 7410 or VOS_PORT_BASE + 10, loopback, a fresh data directory, the test registry; the Calculator and
 *      Agents are installed at start, Notes is only offered; one scripted ACP agent is on the roster);
 *   3. runs vos-host in the foreground, forwarding SIGTERM so the host shuts down gracefully.
 */
import { spawn, spawnSync } from 'node:child_process'
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { CONFIG, DATA, DIR, FAKE_AGENT, FIXTURE_APPS, HOST_BIN, PORT, REGISTRY, ROOT, fixtureAppRef } from './config'

const run = (cmd: string, args: string[]): void => {
  const r = spawnSync(cmd, args, { cwd: ROOT, stdio: 'inherit' })
  if (r.status !== 0) process.exit(r.status ?? 1)
}

/** The version a repo app ships (its app.json), so a bump needs no change here. */
const versionOf = (id: string): string =>
  (JSON.parse(readFileSync(join(ROOT, 'apps', id, 'app.json'), 'utf8')) as { version: string }).version

rmSync(DIR, { recursive: true, force: true })
mkdirSync(DIR, { recursive: true })

run('bun', ['run', '--filter', '@vos/client', 'build'])
run('cargo', ['build', '-q', '-p', 'vos-host'])
run('cargo', ['build', '-q', '-p', 'vos-acp', '--bin', 'fake_agent']) // the scripted agent the Agents journeys talk to

// Notes: the codegen fixture, packaged for the registry like any app
const extra = join(DIR, 'extra-apps/notes')
mkdirSync(extra, { recursive: true })
cpSync(join(ROOT, 'crates/vos-codegen/tests/fixtures/notes/app.luau'), join(extra, 'app.luau'))
writeFileSync(
  join(extra, 'app.json'),
  JSON.stringify({
    id: 'notes',
    name: 'Notes',
    version: '0.1.0',
    description: 'Notes that you and your agents can both write in.',
    icon: 'note.text',
    tint: 'yellow'
  })
)
run('bun', ['scripts/build-apps.ts', '--out', join(DIR, 'registry'), '--root', join(DIR, 'extra-apps')])

// Ticker: built the same way, into a folder of its own that no registry lists (Discover must not offer it)
const hidden = join(DIR, 'hidden-apps/ticker')
mkdirSync(hidden, { recursive: true })
cpSync(join(ROOT, 'crates/vos-host/tests/fixtures/ticker/app.luau'), join(hidden, 'app.luau'))
writeFileSync(
  join(hidden, 'app.json'),
  JSON.stringify({
    id: 'ticker',
    name: 'Ticker',
    version: '0.1.0',
    description: 'A counter that a background task raises on its own.',
    icon: 'clock',
    tint: 'teal'
  })
)
run('bun', ['scripts/build-apps.ts', 'ticker', '--out', join(DIR, 'hidden'), '--root', join(DIR, 'hidden-apps')])

// The framework fixtures: each is built with its own package (its client is a Vite build), into a folder of its own that
// no registry lists, so Discover does not offer them
run('bun', ['scripts/build-apps.ts', ...FIXTURE_APPS, '--out', join(DIR, 'fixtures'), '--root', join(ROOT, 'e2e/apps')])

// Notifier (the notification journeys' sender): the same way, also kept out of the registry
const notifier = join(DIR, 'hidden-apps/notifier')
mkdirSync(notifier, { recursive: true })
cpSync(join(ROOT, 'crates/vos-host/tests/fixtures/notifier/app.luau'), join(notifier, 'app.luau'))
writeFileSync(
  join(notifier, 'app.json'),
  JSON.stringify({
    id: 'notifier',
    name: 'Notifier',
    version: '0.1.0',
    description: 'Buttons that raise notifications, for the notification journeys.',
    icon: 'bell',
    tint: 'pink'
  })
)
run('bun', ['scripts/build-apps.ts', 'notifier', '--out', join(DIR, 'hidden'), '--root', join(DIR, 'hidden-apps')])

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
      ...['calculator', 'agents'].map((id) => `file://${join(DIR, `registry/${id}/${versionOf(id)}/app-ref.json`)}`),
      ...FIXTURE_APPS.map(fixtureAppRef)
    ]
      .map((url) => `"${url}"`)
      .join(', ')}]`,
    // one agent, no model: the scripted ACP agent (crates/vos-acp/src/bin/fake_agent.rs) answers by prompt
    '[[agents]]',
    'id = "scripted"',
    'name = "Scripted agent"',
    'role = "Answers by script, for tests"',
    'avatar = "bolt"',
    'approval = "ask"',
    '[agents.runtime]',
    `command = "${FAKE_AGENT}"`,
    '[agents.grants]',
    'apps = ["calculator"]',
    ''
  ].join('\n')
)

const host = spawn(HOST_BIN, ['--config', CONFIG], { cwd: ROOT, stdio: 'inherit' })
for (const signal of ['SIGINT', 'SIGTERM'] as const) process.on(signal, () => host.kill('SIGTERM'))
host.on('exit', (code) => process.exit(code ?? 0))
