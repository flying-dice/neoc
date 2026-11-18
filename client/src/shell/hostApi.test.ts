import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ago, iconOf, tintBackground } from './hostApi'

const generated = readFileSync(join(import.meta.dir, '../../../types/generated/agentos.ts'), 'utf8')

/** The field names of `export type <name> = { ... }` in the generated TypeScript (these types are flat). */
function fieldsOf(name: string): string[] {
  const start = generated.indexOf(`export type ${name} = {`)
  if (start < 0) throw new Error(`${name} is not in types/generated/agentos.ts`)
  const end = generated.indexOf('\nexport ', start + 1)
  const body = generated
    .slice(start, end < 0 ? undefined : end)
    .replace(/\/\*[^]*?\*\//g, '')
    .replace(/^[^{]*\{/, '')
  return [...body.matchAll(/(?:^|,)\s*(\w+)\??:/g)].map((m) => m[1]).sort()
}

describe('the generated wire types hostApi re-exports', () => {
  const expected: Record<string, string[]> = {
    AppSummary: [
      'id',
      'name',
      'version',
      'state',
      'message',
      'icon',
      'tint',
      'description',
      'window',
      'source'
    ],
    AppWindow: ['w', 'h', 'minW', 'minH'],
    AppPreview: [
      'url',
      'id',
      'name',
      'version',
      'icon',
      'tint',
      'description',
      'source',
      'installed',
      'sha256'
    ],
    InstalledInfo: ['version', 'state'],
    Catalog: ['apps', 'errors'],
    DeviceRow: ['id', 'name', 'createdAt', 'lastSeenAt', 'current'],
    AgentRow: ['id', 'name', 'avatar', 'role', 'runtime', 'grants', 'approval'],
    AgentRuntimeRow: ['command', 'args', 'envKeys']
  }
  for (const [name, fields] of Object.entries(expected)) {
    test(name, () => expect(fieldsOf(name)).toEqual([...fields].sort()))
  }
})

describe('presentation helpers', () => {
  test('an unknown symbol falls back to a generic icon', () => {
    expect(iconOf('plus')).toBe('plus')
    expect(iconOf('no.such.symbol')).toBe('square.grid')
    expect(iconOf(undefined)).toBe('square.grid')
  })
  test('a tint becomes a gradient and an unknown one is blue', () => {
    expect(tintBackground('orange')).toContain('var(--ios-orange)')
    expect(tintBackground('nope')).toContain('var(--ios-blue)')
  })
  test('times read relative', () => {
    expect(ago(1000, 20_000)).toBe('just now')
    expect(ago(0, 3 * 60_000)).toBe('3 minutes ago')
    expect(ago(0, 60_000)).toBe('1 minute ago')
    expect(ago(0, 5 * 3_600_000)).toBe('5 hours ago')
    expect(ago(0, 2 * 86_400_000)).toBe('2 days ago')
  })
})
