import { describe, expect, test } from 'bun:test'
import { GATE_ENV, plan } from './pre-pr'

describe('the gate plan', () => {
  test('is the neoc checks, then lint, typecheck, test and e2e', () => {
    expect(plan([]).map((s) => s.name)).toEqual(['neoc checks', 'lint', 'typecheck', 'test', 'test:e2e'])
  })

  test('--neoc is the scoped script alone, --vos the rest', () => {
    expect(plan(['--neoc']).map((s) => s.cmd)).toEqual([['bash', 'scripts/check-neoc.sh']])
    expect(plan(['--vos']).map((s) => s.name)).toEqual(['lint', 'typecheck', 'test', 'test:e2e'])
  })

  test('--no-e2e leaves out test:e2e and nothing else, alone or with --vos', () => {
    const lint = ['neoc checks', 'lint', 'typecheck', 'test']
    expect(plan(['--no-e2e']).map((s) => s.name)).toEqual(lint)
    expect(plan(['--vos', '--no-e2e']).map((s) => s.name)).toEqual(lint.slice(1))
    expect(plan(['--neoc', '--no-e2e']).map((s) => s.name)).toEqual(['neoc checks'])
  })

  test('an unknown flag is refused rather than ignored', () => {
    expect(() => plan(['--no-e2e-please'])).toThrow('unknown argument')
  })

  test('both flags together are refused rather than guessed at', () => {
    expect(() => plan(['--neoc', '--vos'])).toThrow('whole gate')
  })

  test('every step runs with both "a missing tool fails" switches', () => {
    expect(GATE_ENV).toEqual({ VOS_GATE: '1', NEOC_REQUIRE_LUAU_LSP: '1' })
  })
})
