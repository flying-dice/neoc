import { describe, expect, test } from 'bun:test'
import { keyProblem, nameProblem } from './validate'

describe('a project name', () => {
  test('will do when it is short enough, and when it is still blank', () => {
    expect(nameProblem('Website')).toBeNull()
    expect(nameProblem('   ')).toBeNull()
  })

  test('is too long past 60 characters', () => {
    expect(nameProblem('x'.repeat(61))).toBe('A name is at most 60 characters.')
  })
})

describe('a project key', () => {
  test.each(['WEB', 'web', 'A1', 'ABCDE', ' ops ', ''])('%p will do', (key) => {
    expect(keyProblem(key)).toBeNull()
  })

  test.each(['W', '1AB', 'ABCDEF', 'A-B', 'A B'])('%p is not a key', (key) => {
    expect(keyProblem(key)).toBe('A key is two to five letters or digits, a letter first, like WEB.')
  })
})
