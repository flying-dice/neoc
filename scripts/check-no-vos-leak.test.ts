import { afterAll, describe, expect, test } from 'bun:test'
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const script = resolve(import.meta.dir, 'check-no-vos-leak.sh')
const dirs: string[] = []

/** Runs the check with a stub `cargo` first on PATH that prints `output` and exits with `code`. */
function run(output: string, code: number): { status: number; stderr: string } {
  const dir = mkdtempSync(join(tmpdir(), 'no-vos-leak-'))
  dirs.push(dir)
  const cargo = join(dir, 'cargo')
  writeFileSync(cargo, `#!/usr/bin/env bash\ncat <<'TREE'\n${output}\nTREE\nexit ${code}\n`)
  chmodSync(cargo, 0o755)
  const proc = Bun.spawnSync(['bash', script], { env: { ...process.env, PATH: `${dir}:${process.env.PATH}` } })
  return { status: proc.exitCode ?? -1, stderr: proc.stderr.toString() }
}

afterAll(() => {
  for (const d of dirs) rmSync(d, { recursive: true, force: true })
})

describe('the no-vos-leak check', () => {
  test('passes on a tree without a vos crate', () => {
    expect(run('neoc v0.1.0\nneoc-vm v0.1.0\nmlua v0.12.2', 0).status).toBe(0)
  })

  test('fails when a vos crate is in the tree', () => {
    const r = run('neoc v0.1.0\nneoc-vm v0.1.0\nvos-domain v0.1.0', 0)
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('must not depend on a vos-* crate')
  })

  test('fails when cargo tree itself fails, even if it printed a clean-looking tree', () => {
    const r = run('neoc v0.1.0\nneoc-vm v0.1.0', 101)
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('cargo tree failed')
  })

  test('fails on an empty tree rather than reading it as "no leak"', () => {
    const r = run('', 0)
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('did not list neoc-vm')
  })
})
