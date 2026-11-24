/**
 * The Luau tests of `vos:docker` (`crates/vos-lua/lua/docker/*.test.lua`): each is bundled with what it requires, as
 * the framework itself is (darklua: scripts/luau.ts), and run by neoc, whose `lib:test` they are written with. Only
 * modules that need nothing of vos can be tested this way (`vos:app` is the host's): what talks to Docker is covered by
 * the host's own tests and the journeys of the apps that use it.
 */
import { expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { requireDarklua, root, toLuau } from './luau'

const dir = join(root, 'crates/vos-lua/lua/docker')
const suites = readdirSync(dir)
  .filter((name) => name.endsWith('.test.lua'))
  .sort()

test('the framework has Luau tests', () => {
  expect(suites.length).toBeGreaterThan(0)
})

test.each(suites)(
  '%s passes',
  (suite) => {
    const scratch = mkdtempSync(join(tmpdir(), 'vos-docker-test-'))
    try {
      const bundle = join(scratch, suite)
      writeFileSync(bundle, toLuau(requireDarklua(), join(dir, suite)))
      const run = spawnSync('cargo', ['run', '-q', '-p', 'neoc', '--', 'run', bundle], { cwd: root, encoding: 'utf8' })
      expect(`${run.stdout}${run.stderr}`).toContain('passed')
      expect(`${run.stdout}${run.stderr}`).not.toContain('failed')
      expect(run.status).toBe(0)
    } finally {
      rmSync(scratch, { recursive: true, force: true })
    }
  },
  600_000
)
