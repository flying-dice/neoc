import { describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { inside, refuseRelativeFile } from './paths.ts'
import { canLink } from './test-support.ts'

describe('a file name from the contract', () => {
  test('plain relative names are fine', () => {
    for (const ok of ['001_init.sql', 'migrations/001_init.sql', 'a/b/c-d.e_f.sql', 'x.sql']) expect(refuseRelativeFile(ok), ok).toBeNull()
  })
  test('anything that could leave the project or is not a plain name is refused with a reason', () => {
    const bad = [
      '', '../x', '../../x', 'a/../../x', 'a/..', '..', '/etc/passwd', '//server/share', 'C:/x', 'C:\\x', 'a\\b', 'a//b', './a', 'a/./b',
      '.hidden', 'a/.git/x', 'a\0b', 'a b', 'a;b', 'é.sql', '%2e%2e/x', 'a/..%2fb'
    ]
    for (const name of bad) expect(refuseRelativeFile(name), JSON.stringify(name)).not.toBeNull()
  })
})

describe('resolving inside a root', () => {
  test.skipIf(!canLink)('stays inside, whatever the name, and follows no symbolic link out', () => {
    const root = mkdtempSync(join(tmpdir(), 'vos-paths-'))
    const outside = mkdtempSync(join(tmpdir(), 'vos-outside-'))
    try {
      mkdirSync(join(root, 'migrations'))
      writeFileSync(join(root, 'migrations/001.sql'), 'x')
      writeFileSync(join(outside, 'secret.txt'), 'x')
      symlinkSync(outside, join(root, 'link'))
      expect(inside(root, 'migrations/001.sql')).toBe(join(root, 'migrations/001.sql'))
      expect(inside(root, 'not/yet/there.sql')).toBe(join(root, 'not/yet/there.sql'))
      for (const name of ['../x', '../../x', '/etc/passwd', 'migrations/../../x', 'link/secret.txt', 'link/new.txt']) {
        expect(() => inside(root, name), name).toThrow(/outside/)
      }
    } finally {
      rmSync(root, { recursive: true, force: true })
      rmSync(outside, { recursive: true, force: true })
    }
  })
})
