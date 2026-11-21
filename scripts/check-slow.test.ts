import { afterAll, describe, expect, test } from 'bun:test'
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const script = resolve(import.meta.dir, 'check-slow.sh')
const dirs: string[] = []

/** The ignored tests of the workspace today, as `cargo nextest list --run-ignored only --message-format oneline` prints them. */
const TODAY = [
  'neoc-vm lua::serde_guard::tests::a_wide_value_is_walked_whole',
  'neoc-vm::hostile_inputs a_ring_of_sixty_thousand_macros_is_planned_in_linear_time',
  'neoc-vm::hostile_inputs no_module_function_panics_on_hostile_input',
  'vos-domain ui::diff::tests::apply_of_diff_reproduces_the_new_tree_for_edited_trees_full',
  'vos-domain ui::diff::tests::apply_of_diff_reproduces_the_new_tree_for_unrelated_trees_full',
  'vos-lua::baseline calculator_vm_create_time_and_memory'
]

/**
 * Runs `check-slow.sh --list` (the selection and its cross-check, no slow suite) against the real `crates/` with a stub
 * `cargo` first on PATH that prints `listed` as the ignored tests and exits with `code`.
 */
function list(listed: string[], code = 0): { status: number; stdout: string; stderr: string } {
  const dir = mkdtempSync(join(tmpdir(), 'check-slow-'))
  dirs.push(dir)
  const cargo = join(dir, 'cargo')
  writeFileSync(cargo, `#!/usr/bin/env bash\ncat <<'LIST'\n${listed.join('\n')}\nLIST\nexit ${code}\n`)
  chmodSync(cargo, 0o755)
  const proc = Bun.spawnSync(['bash', script, '--list'], { env: { ...process.env, PATH: `${dir}:${process.env.PATH}` } })
  return { status: proc.exitCode ?? -1, stdout: proc.stdout.toString(), stderr: proc.stderr.toString() }
}

afterAll(() => {
  for (const d of dirs) rmSync(d, { recursive: true, force: true })
})

describe('the slow-suite selection', () => {
  test('selects the five slow: tests and leaves out the baseline measurement', () => {
    const r = list(TODAY)
    expect(r.status).toBe(0)
    expect(r.stdout).toContain('slow tests (5)')
    expect(r.stdout).toContain('no_module_function_panics_on_hostile_input')
    expect(r.stdout).not.toContain('calculator_vm_create_time_and_memory')
  })

  test('stops on an ignored test that is neither slow: nor on NOT_SLOW (another spelling, or no reason)', () => {
    const r = list([...TODAY, 'vos-lua::views a_test_ignored_some_other_way'])
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('vos-lua::views a_test_ignored_some_other_way')
  })

  test('stops on a slow: test that nextest does not list as ignored', () => {
    const r = list(TODAY.filter((t) => !t.endsWith(' no_module_function_panics_on_hostile_input')))
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('does not list as ignored: neoc-vm::hostile_inputs no_module_function_panics_on_hostile_input')
  })

  test('selects each slow: test by binary id and full path', () => {
    const r = list(TODAY)
    expect(r.stdout).toContain(
      '(binary_id(=vos-domain) & test(=ui::diff::tests::apply_of_diff_reproduces_the_new_tree_for_edited_trees_full))'
    )
    expect(r.stdout).not.toContain('test(/')
  })

  test('does not take an ignored test in another binary with the same last name for the slow: one', () => {
    const r = list([...TODAY, 'vos-lua::views no_module_function_panics_on_hostile_input'])
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('nor on NOT_SLOW: vos-lua::views no_module_function_panics_on_hostile_input')
  })

  test('stops when the slow: test is missing and a test of its name is ignored in another binary', () => {
    const r = list([
      ...TODAY.filter((t) => !t.endsWith(' no_module_function_panics_on_hostile_input')),
      'vos-lua::views no_module_function_panics_on_hostile_input'
    ])
    expect(r.status).toBe(1)
    expect(r.stdout).not.toContain('slow tests (')
  })

  test('stops when two ignored tests of one binary end in the same slow: name', () => {
    const r = list([...TODAY, 'neoc-vm::hostile_inputs other::no_module_function_panics_on_hostile_input'])
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('two ignored tests in neoc-vm::hostile_inputs end in no_module_function_panics_on_hostile_input')
  })

  test('stops when cargo nextest list fails', () => {
    const r = list(TODAY, 101)
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('cargo nextest list failed')
  })

  test('refuses an unknown argument', () => {
    expect(Bun.spawnSync(['bash', script, '--all']).exitCode).toBe(2)
  })
})
