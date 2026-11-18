#!/usr/bin/env bun
/**
 * Starts the real host for the end-to-end specs (Playwright's webServer runs this):
 *
 *   1. builds the client (client/dist) and the host (cargo), and the apps into a test registry:
 *      the repo's apps plus Notes (the codegen fixture), a second app the Discover journeys install and remove
 *      so they never disturb the Calculator the other specs use;
 *   2. writes agentos.toml (port 7410 or AGENTOS_PORT_BASE + 10, loopback, a fresh data directory, the test registry; the Calculator and
 *      Agents are installed at start, Notes is only offered; one scripted ACP agent is on the roster);
 *   3. runs agentos-host in the foreground, forwarding SIGTERM so the host shuts down gracefully.
 */
import { spawn, spawnSync } from 'node:child_process'
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { CONFIG, DATA, DIR, FAKE_AGENT, HOST_BIN, PORT, REGISTRY, ROOT } from './config'

const run = (cmd: string, args: string[]): void => {
  const r = spawnSync(cmd, args, { cwd: ROOT, stdio: 'inherit' })
  if (r.status !== 0) process.exit(r.status ?? 1)
}

/** The version a repo app ships (its app.json), so a bump needs no change here. */
const versionOf = (id: string): string =>
  (JSON.parse(readFileSync(join(ROOT, 'apps', id, 'app.json'), 'utf8')) as { version: string }).version

rmSync(DIR, { recursive: true, force: true })
mkdirSync(DIR, { recursive: true })

run('bun', ['run', '--filter', '@agentos/client', 'build'])
run('cargo', ['build', '-q', '-p', 'agentos-host'])
run('cargo', ['build', '-q', '-p', 'agentos-acp', '--bin', 'fake_agent']) // the scripted agent the Agents journeys talk to

// Notes: the codegen fixture, packaged for the registry like any app
const extra = join(DIR, 'extra-apps/notes')
mkdirSync(extra, { recursive: true })
cpSync(join(ROOT, 'crates/agentos-codegen/tests/fixtures/notes/app.lua'), join(extra, 'app.lua'))
writeFileSync(
  join(extra, 'app.json'),
  JSON.stringify({
    id: 'notes',
    name: 'Notes',
    version: '0.1.0',
    description: 'Notes that you and your agents can both write in.',
    icon: 'note.text',
    tint: 'yellow',
    permissions: ['data', 'notify']
  })
)
run('bun', ['scripts/build-apps.ts', '--out', join(DIR, 'registry'), '--root', join(DIR, 'extra-apps')])

mkdirSync(DATA, { recursive: true })
writeFileSync(
  CONFIG,
  [
    `data_dir = "${DATA}"`,
    `bind = "127.0.0.1"`,
    `port = ${PORT}`,
    `client_dir = "${join(ROOT, 'client/dist')}"`,
    `registries = ["file://${REGISTRY}"]`,
    `install = [${['calculator', 'agents'].map((id) => `"file://${join(DIR, `registry/${id}/${versionOf(id)}/app-ref.json`)}"`).join(', ')}]`,
    // one agent, no model: the scripted ACP agent (crates/agentos-acp/src/bin/fake_agent.rs) answers by prompt
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
