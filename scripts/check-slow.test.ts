import { afterAll, describe, expect, test } from 'bun:test'
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'

const script = resolve(import.meta.dir, 'check-slow.sh')
const dirs: string[] = []

/**
 * A workspace of its own for the script to read. The script looks for `slow:` tests under the `crates/` next to its own
 * `scripts/`, so the copy below reads these files and never the real workspace. These tests are about the selection,
 * not about how many slow tests the repository has today. The real cross-check is the script itself:
 * `bash scripts/check-slow.sh --list` (and `bun run slow`) fails when its selection and nextest disagree. The fixture
 * uses the real shapes: an integration test file, a library module with the attribute indented in `mod tests`, and a
 * `src/` file of another crate.
 */
const FIXTURE: Record<string, string> = {
  'crates/neoc-vm/Cargo.toml': '[package]\nname = "neoc-vm"\n',
  'crates/neoc-vm/tests/hostile_inputs.rs': [
    '#[test]',
    '#[ignore = "slow: about two million hostile calls"]',
    'fn no_module_function_panics_on_hostile_input() {}',
    '',
    '#[test]',
    '#[ignore = "slow: a wall-clock bound"]',
    'fn a_ring_of_sixty_thousand_macros_is_planned_in_linear_time() {}',
    ''
  ].join('\n'),
  'crates/neoc-vm/src/lua/serde_guard.rs': [
    'mod tests {',
    '    #[test]',
    '    #[ignore = "slow: builds over a million tables"]',
    '    fn a_wide_value_is_walked_whole() {}',
    '}',
    ''
  ].join('\n'),
  // a module crate sits one level down, in `crates/neoc-vm-lib/<module>`
  'crates/neoc-vm-lib/template/Cargo.toml': '[package]\nname = "neoc-vm-lib-template"\n',
  'crates/neoc-vm-lib/template/src/lib.rs': [
    'mod tests {',
    '    #[test]',
    '    #[ignore = "slow: a ring of macros"]',
    '    fn a_ring_is_planned_in_linear_time() {}',
    '}',
    ''
  ].join('\n'),
  'crates/vos-domain/Cargo.toml': '[package]\nname = "vos-domain"\n',
  'crates/vos-domain/src/ui/diff_tests.rs': [
    '#[test]',
    '#[ignore = "slow: 1500 generated trees"]',
    'fn apply_of_diff_reproduces_the_new_tree_for_unrelated_trees_full() {}',
    '',
    '#[test]',
    '#[ignore = "slow: 3000 generated trees"]',
    'fn apply_of_diff_reproduces_the_new_tree_for_edited_trees_full() {}',
    ''
  ].join('\n')
}

/** The fixture's slow: tests, as `cargo nextest list --run-ignored only --message-format oneline` prints them. */
const SLOW = [
  'neoc-vm-lib-template tests::a_ring_is_planned_in_linear_time',
  'neoc-vm lua::serde_guard::tests::a_wide_value_is_walked_whole',
  'neoc-vm::hostile_inputs a_ring_of_sixty_thousand_macros_is_planned_in_linear_time',
  'neoc-vm::hostile_inputs no_module_function_panics_on_hostile_input',
  'vos-domain ui::diff::tests::apply_of_diff_reproduces_the_new_tree_for_edited_trees_full',
  'vos-domain ui::diff::tests::apply_of_diff_reproduces_the_new_tree_for_unrelated_trees_full'
]
/** Everything nextest lists as ignored in the fixture: the slow: tests and the one on the script's NOT_SLOW list. */
const IGNORED = [...SLOW, 'vos-lua::it baseline::calculator_vm_create_time_and_memory']

/**
 * Runs `check-slow.sh --list` (the selection and its cross-check, no slow suite) in a fresh copy of the fixture, with a
 * stub `cargo` first on PATH that prints `listed` as the ignored tests and exits with `code`.
 */
function list(listed: string[], code = 0): { status: number; stdout: string; stderr: string } {
  const root = mkdtempSync(join(tmpdir(), 'check-slow-'))
  dirs.push(root)
  for (const [path, text] of Object.entries(FIXTURE)) {
    mkdirSync(dirname(join(root, path)), { recursive: true })
    writeFileSync(join(root, path), text)
  }
  mkdirSync(join(root, 'scripts'))
  copyFileSync(script, join(root, 'scripts/check-slow.sh'))
  const bin = join(root, 'bin')
  mkdirSync(bin)
  writeFileSync(join(bin, 'cargo'), `#!/usr/bin/env bash\ncat <<'LIST'\n${listed.join('\n')}\nLIST\nexit ${code}\n`)
  chmodSync(join(bin, 'cargo'), 0o755)
  const proc = Bun.spawnSync(['bash', join(root, 'scripts/check-slow.sh'), '--list'], {
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}` }
  })
  return { status: proc.exitCode ?? -1, stdout: proc.stdout.toString(), stderr: proc.stderr.toString() }
}

afterAll(() => {
  for (const d of dirs) rmSync(d, { recursive: true, force: true })
})

// the script is bash and its stub `cargo` a shell script first on a `:` PATH: Unix only (on Windows `bash` is WSL's)
describe.skipIf(process.platform === 'win32')('the slow-suite selection', () => {
  test('selects every slow: test and leaves out the baseline measurement', () => {
    const r = list(IGNORED)
    expect(r.status).toBe(0)
    expect(r.stdout).toContain(`slow tests (${SLOW.length})`)
    for (const t of SLOW) expect(r.stdout).toContain(t)
    expect(r.stdout).not.toContain('calculator_vm_create_time_and_memory')
  })

  test('stops on an ignored test that is neither slow: nor on NOT_SLOW (another spelling, or no reason)', () => {
    const r = list([...IGNORED, 'vos-lua::views a_test_ignored_some_other_way'])
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('vos-lua::views a_test_ignored_some_other_way')
  })

  test('stops on a slow: test that nextest does not list as ignored', () => {
    const r = list(IGNORED.filter((t) => !t.endsWith(' no_module_function_panics_on_hostile_input')))
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('does not list as ignored: neoc-vm::hostile_inputs no_module_function_panics_on_hostile_input')
  })

  test('selects each slow: test by binary id and full path', () => {
    const r = list(IGNORED)
    expect(r.stdout).toContain(
      '(binary_id(=vos-domain) & test(=ui::diff::tests::apply_of_diff_reproduces_the_new_tree_for_edited_trees_full))'
    )
    expect(r.stdout).toContain('(binary_id(=neoc-vm) & test(=lua::serde_guard::tests::a_wide_value_is_walked_whole))')
    expect(r.stdout).toContain('(binary_id(=neoc-vm-lib-template) & test(=tests::a_ring_is_planned_in_linear_time))')
    expect(r.stdout).not.toContain('test(/')
  })

  test('does not take an ignored test in another binary with the same last name for the slow: one', () => {
    const r = list([...IGNORED, 'vos-lua::views no_module_function_panics_on_hostile_input'])
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('nor on NOT_SLOW: vos-lua::views no_module_function_panics_on_hostile_input')
  })

  test('stops when the slow: test is missing and a test of its name is ignored in another binary', () => {
    const r = list([
      ...IGNORED.filter((t) => !t.endsWith(' no_module_function_panics_on_hostile_input')),
      'vos-lua::views no_module_function_panics_on_hostile_input'
    ])
    expect(r.status).toBe(1)
    expect(r.stdout).not.toContain('slow tests (')
  })

  test('stops when two ignored tests of one binary end in the same slow: name', () => {
    const r = list([...IGNORED, 'neoc-vm::hostile_inputs other::no_module_function_panics_on_hostile_input'])
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('two ignored tests in neoc-vm::hostile_inputs end in no_module_function_panics_on_hostile_input')
  })

  test('stops when cargo nextest list fails', () => {
    const r = list(IGNORED, 101)
    expect(r.status).toBe(1)
    expect(r.stderr).toContain('cargo nextest list failed')
  })

  test('refuses an unknown argument', () => {
    expect(Bun.spawnSync(['bash', script, '--all']).exitCode).toBe(2)
  })
})
