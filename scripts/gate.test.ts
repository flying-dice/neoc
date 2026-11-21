import { describe, expect, test } from 'bun:test'
import { isGateRun, missingTool } from './gate'

describe('a missing lint tool', () => {
  test('skips on a developer machine', () => {
    expect(missingTool('lint:luau', 'luau-lsp not found', {})).toEqual({
      line: 'lint:luau SKIPPED: luau-lsp not found',
      code: 0
    })
  })

  test('fails in the gate (VOS_GATE=1)', () => {
    const { line, code } = missingTool('lint:luau', 'luau-lsp not found', { VOS_GATE: '1' })
    expect(code).toBe(1)
    expect(line).toContain('lint:luau FAILED: luau-lsp not found')
  })

  test('fails under CI', () => {
    expect(missingTool('lint:stylua', 'stylua not found', { CI: 'true' }).code).toBe(1)
  })
})

describe('what counts as the gate', () => {
  test.each([
    [{ VOS_GATE: '1' }, true],
    [{ CI: 'true' }, true],
    [{ CI: '1' }, true],
    [{}, false],
    [{ VOS_GATE: '0' }, false],
    [{ CI: '' }, false],
    [{ CI: 'false' }, false],
    [{ CI: '0' }, false]
  ])('%o is %p', (env, expected) => {
    expect(isGateRun(env)).toBe(expected)
  })
})
