import { describe, expect, test } from 'bun:test'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { elementsOf, kitTypes, type Manifest } from '../packages/vite-plugin/src/kit-types.ts'

const KIT = resolve(import.meta.dirname, '../packages/kit')
const FIXTURES = join(KIT, 'fixtures')

/** Runs a command in the kit package and returns its exit code and output. */
function run(cmd: string[]): { code: number; out: string } {
  const r = Bun.spawnSync(cmd, { cwd: KIT, stdout: 'pipe', stderr: 'pipe' })
  return { code: r.exitCode, out: `${r.stdout}${r.stderr}` }
}

// the manifest the way the build makes it, into a folder of the fixtures (not the served kit)
const generated = join(FIXTURES, 'generated')
mkdirSync(generated, { recursive: true })
const analyze = run(['node_modules/.bin/cem', 'analyze', '--config', 'custom-elements-manifest.config.mjs', '--outdir', 'fixtures/generated'])
const manifest = JSON.parse(readFileSync(join(generated, 'custom-elements.json'), 'utf8')) as Manifest

describe('the Custom Elements Manifest', () => {
  test('is made by the analyzer', () => {
    expect(analyze.code, analyze.out).toBe(0)
  })

  test('the committed copy the vos Vite plugin reads (packages/kit/custom-elements.json) is the one the kit makes', () => {
    const committed = readFileSync(join(KIT, 'custom-elements.json'), 'utf8')
    expect(committed === readFileSync(join(generated, 'custom-elements.json'), 'utf8'), 'stale: run `bun run build:kit` and commit packages/kit/custom-elements.json').toBe(true)
  })

  test('describes every tag the kit defines, with its attributes, events and slots', () => {
    const tags = elementsOf(manifest).map((e) => e.tagName)
    expect(tags).toEqual([
      'ui-alert', 'ui-banner', 'ui-button', 'ui-check', 'ui-empty-state', 'ui-icon', 'ui-list', 'ui-meter', 'ui-picker', 'ui-row', 'ui-section',
      'ui-segmented', 'ui-spinner', 'ui-split-view', 'ui-text-area', 'ui-text-field', 'ui-tile', 'ui-toggle', 'ui-value-pair'
    ].sort((a, b) => a.localeCompare(b)))
    const byTag = Object.fromEntries(elementsOf(manifest).map((e) => [e.tagName, e]))
    expect(byTag['ui-button']!.attributes!.map((a) => a.name)).toContain('variant')
    expect(byTag['ui-toggle']!.events!.map((e) => e.name)).toEqual(['change']) // inherited from the shared base class
    expect(byTag['ui-toggle']!.events![0]!.type!.text).toBe('CustomEvent<{ checked: boolean }>')
    expect(byTag['ui-row']!.slots!.map((s) => s.name)).toEqual(['', 'leading', 'trailing'])
    expect(byTag['ui-alert']!.events!.map((e) => e.name)).toEqual(['confirm', 'cancel'])
    expect(byTag['ui-split-view']!.slots!.map((s) => s.name)).toEqual(['sidebar', 'content', ''])
    expect(byTag['ui-split-view']!.events![0]!.type!.text).toBe('CustomEvent<{ collapsed: boolean }>')
    expect(byTag['ui-picker']!.events!.find((e) => e.name === 'change')!.type!.text).toBe(
      'CustomEvent<{ value: string }>'
    )
    expect(byTag['ui-text-area']!.events!.map((e) => e.name)).toEqual(['input', 'change'])
  })

  test('every attribute of every tag has a type and a description (the editor shows them)', () => {
    for (const el of elementsOf(manifest))
      for (const a of el.attributes ?? []) {
        expect(a.type?.text, `${el.tagName} ${a.name} has no type`).toBeTruthy()
        expect(a.description, `${el.tagName} ${a.name} has no description`).toBeTruthy()
      }
  })
})

describe('the editor types', () => {
  for (const framework of ['react', 'svelte'] as const) {
    mkdirSync(join(FIXTURES, framework, 'generated'), { recursive: true })
    writeFileSync(join(FIXTURES, framework, 'generated', `kit.${framework}.d.ts`), kitTypes(manifest, framework))
  }

  test('a React app gets them: what it writes type-checks, a wrong attribute or event does not', () => {
    const r = run(['node_modules/.bin/tsc', '-p', 'fixtures/react/tsconfig.json'])
    expect(r.out).toBe('')
    expect(r.code).toBe(0)
  })

  test('a Svelte app gets them: what it writes type-checks, a wrong attribute or event does not', () => {
    const r = run(['node_modules/.bin/svelte-check', '--workspace', 'fixtures/svelte', '--tsconfig', './tsconfig.json', '--output', 'machine'])
    const errors = [...r.out.matchAll(/ERROR "([^"]+)" (\d+):\d+ /g)].map((m) => ({ file: m[1], line: Number(m[2]) }))
    expect(errors.filter((e) => e.file === 'Usage.svelte')).toEqual([])
    // one error on each element of Wrong.svelte, and no other
    const wrong = readFileSync(join(FIXTURES, 'svelte/Wrong.svelte'), 'utf8').split('\n')
    const elements = wrong.flatMap((l, i) => (l.startsWith('<ui-') ? [i + 1] : []))
    expect(errors.filter((e) => e.file === 'Wrong.svelte').map((e) => e.line)).toEqual(elements)
    expect(errors.length).toBe(elements.length)
  })

  test('the types of a tag follow its manifest entry (an attribute added to a component appears)', () => {
    const extra: Manifest = JSON.parse(JSON.stringify(manifest))
    elementsOf(extra)[0]!.attributes!.push({ name: 'brand-new', type: { text: 'number' }, description: 'New.' })
    expect(kitTypes(extra, 'react')).toContain('"brand-new"?: number')
    expect(kitTypes(extra, 'svelte')).toContain('"brand-new"?: number')
  })
})
