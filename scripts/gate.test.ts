import { describe, expect, test } from 'bun:test'
import { isGateRun, missingTool } from './gate'

describe('a missing lint tool', () => {
  test('skips on a developer machine', () => {
    expect(missingTool('lint:lua', 'lua-language-server not found', {})).toEqual({
      line: 'lint:lua SKIPPED: lua-language-server not found',
      code: 0
    })
  })

  test('fails in the gate (AGENTOS_GATE=1)', () => {
    const { line, code } = missingTool('lint:lua', 'lua-language-server not found', { AGENTOS_GATE: '1' })
    expect(code).toBe(1)
    expect(line).toContain('lint:lua FAILED: lua-language-server not found')
  })

  test('fails under CI', () => {
    expect(missingTool('lint:stylua', 'stylua not found', { CI: 'true' }).code).toBe(1)
  })
})

describe('what counts as the gate', () => {
  test.each([
    [{ AGENTOS_GATE: '1' }, true],
    [{ CI: 'true' }, true],
    [{ CI: '1' }, true],
    [{}, false],
    [{ AGENTOS_GATE: '0' }, false],
    [{ CI: '' }, false],
    [{ CI: 'false' }, false],
    [{ CI: '0' }, false]
  ])('%o is %p', (env, expected) => {
    expect(isGateRun(env)).toBe(expected)
  })
})
