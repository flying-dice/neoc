import { describe, expect, test } from 'bun:test'
import { applyPatch, PatchError } from './jsonPatch'

describe('applyPatch (RFC 6902 subset)', () => {
  const doc = {
    session: { usage: null, commands: [{ name: 'a' }] },
    plan: null,
    'a/b': { '~x': 1 }
  }

  test('replace and add set members, and the input is not mutated', () => {
    const out = applyPatch(doc, [
      { op: 'replace', path: '/session/usage', value: { used: 1, size: 2 } },
      { op: 'add', path: '/session/title', value: 'Hi' }
    ])
    expect(out.session).toMatchObject({ usage: { used: 1, size: 2 }, title: 'Hi' })
    expect(doc.session.usage).toBeNull()
  })

  test('remove deletes a member or an array element', () => {
    const out = applyPatch(doc, [
      { op: 'remove', path: '/plan' },
      { op: 'remove', path: '/session/commands/0' }
    ])
    expect('plan' in out).toBe(false)
    expect(out.session.commands).toEqual([])
  })

  test('add into an array inserts at an index or appends with "-"', () => {
    const out = applyPatch(doc, [
      { op: 'add', path: '/session/commands/-', value: { name: 'c' } },
      { op: 'add', path: '/session/commands/0', value: { name: 'z' } }
    ])
    expect(out.session.commands.map((c) => c.name)).toEqual(['z', 'a', 'c'])
  })

  test('pointer escapes: ~1 is a slash and ~0 a tilde', () => {
    const out = applyPatch(doc, [{ op: 'replace', path: '/a~1b/~0x', value: 2 }])
    expect(out['a/b']).toEqual({ '~x': 2 })
  })

  test('an empty path replaces the whole document', () => {
    expect(applyPatch<unknown>(doc, [{ op: 'replace', path: '', value: { fresh: true } }])).toEqual(
      { fresh: true }
    )
  })

  test('test checks a value', () => {
    expect(() => applyPatch(doc, [{ op: 'test', path: '/plan', value: null }])).not.toThrow()
    expect(() => applyPatch(doc, [{ op: 'test', path: '/plan', value: 1 }])).toThrow(PatchError)
  })

  test('operations that cannot apply are errors, not silent no-ops', () => {
    for (const op of [
      { op: 'replace', path: '/missing', value: 1 },
      { op: 'remove', path: '/session/nope' },
      { op: 'add', path: '/session/commands/9', value: 1 },
      { op: 'add', path: '/nowhere/deep', value: 1 },
      { op: 'move', path: '/plan', from: '/session' },
      { op: 'add', path: 'no-slash', value: 1 }
    ] as const)
      expect(() => applyPatch(doc, [op])).toThrow(PatchError)
  })
})
