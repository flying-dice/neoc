import { describe, expect, test } from 'bun:test'
import { CODER, ECHO, SCRIPTED } from './fakeApi'
import { KINDS, folderFor, needsDocker, subheadingOf, takes } from './kinds'

describe('the types of agent', () => {
  test('are offered in the order remote, container, program, each with an icon and a line that says what it is', () => {
    expect(KINDS.map((k) => k.kind)).toEqual(['remote', 'container', 'program'])
    expect(KINDS.map((k) => k.label)).toEqual(['Remote', 'Container', 'Program'])
    for (const kind of KINDS) {
      expect(kind.icon).not.toBe('')
      expect(kind.explanation).not.toBe('')
    }
  })

  test('take what only they have: an address, a command, an image', () => {
    expect(takes('remote')).toEqual({ address: true, command: false, image: false })
    expect(takes('program')).toEqual({ address: false, command: true, image: false })
    expect(takes('container')).toEqual({ address: false, command: true, image: true })
  })

  test("start in the folder their type has when none is given; a program's has to be chosen", () => {
    expect(folderFor('remote')).toBe('/')
    expect(folderFor('container')).toBe('/workspace')
    expect(folderFor('program')).toBe('')
  })

  test('need Docker only for a container', () => {
    expect(needsDocker('container')).toBe(true)
    expect(needsDocker('remote')).toBe(false)
    expect(needsDocker('program')).toBe(false)
    expect(needsDocker(undefined)).toBe(false)
  })
})

describe("an agent's line in the sidebar", () => {
  test('is its address, Container, or its command', () => {
    expect(subheadingOf(SCRIPTED)).toBe(SCRIPTED.url)
    expect(subheadingOf(CODER)).toBe('Container')
    expect(subheadingOf(ECHO)).toBe('python3 -u echo_agent.py')
    expect(subheadingOf({ ...ECHO, command: ['agent', '--name', 'two words'] })).toBe('agent --name "two words"')
  })
})
