import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import {
  allowedRequire,
  checkNegative,
  dropKnownUnknownRequires,
  DEFINITIONS,
  expectedErrors,
  finding,
  inViteApp,
  markerCount,
  mergeFindings,
  NEGATIVE_MARKERS,
  offeredModules,
  overloads,
  parseAnalysis,
  requiredNames,
  withoutRequire
} from './lint-luau'

const root = join(import.meta.dir, '..')

describe('the matcher that holds the negative fixtures to their markers', () => {
  test('accepts exactly the marked findings', () => {
    const source = 'a() -- expect-error: not found\nb()\nc() -- expect-error: one -- expect-error: two\n'
    const findings = [finding(1, 'TypeError: not found'), finding(3, 'TypeError: number one'), finding(3, 'TypeError: two')]
    expect(checkNegative('f', source, findings)).toEqual([])
  })

  test('refuses a marker that only the caused-by lines satisfy', () => {
    const source = "f(1) -- expect-error: Expected this to be 'string', but got 'number'\n"
    const optional = finding(
      1,
      "TypeError: Expected this to be 'string?', but got 'number'\ncaused by:\n  None of the union options are compatible. For example:\nExpected this to be 'string', but got 'number'"
    )
    expect(checkNegative('f', source, [optional])).not.toEqual([])
    expect(checkNegative('f', source, [finding(1, "TypeError: Expected this to be 'string', but got 'number'")])).toEqual([])
  })

  test('refuses extra, missing and unmarked findings', () => {
    const source = 'a() -- expect-error: first\nb()\nc() -- expect-error: third\n'
    // a second finding on a marked line
    expect(checkNegative('f', source, [finding(1, 'first'), finding(1, 'another'), finding(3, 'third')])).toHaveLength(1)
    // a marked line luau-lsp accepted
    expect(checkNegative('f', source, [finding(1, 'first')])).toHaveLength(1)
    // a finding on an unmarked line
    const problems = checkNegative('f', source, [finding(1, 'first'), finding(2, 'surprise'), finding(3, 'third')])
    expect(problems).toHaveLength(1)
    expect(problems[0]).toContain('not marked')
    // two markers need two different findings
    expect(
      checkNegative('f', 'a() -- expect-error: first -- expect-error: first\n', [finding(1, 'first'), finding(1, 'other')])
    ).toHaveLength(1)
    // a fixture without markers expects nothing
    expect(checkNegative('f', 'a()\n', [])).toHaveLength(1)
  })

  test('a marker needs its text', () => {
    expect(() => expectedErrors('f', 'a() -- expect-error\n')).toThrow('needs a `: <text>`')
  })
})

describe('what luau-lsp prints', () => {
  const files = ['apps/x/app.luau', 'scripts/fixtures/negative/y.luau']
  test('findings are grouped by file, a message continuing on the lines after it', () => {
    const output = [
      '[INFO] Loading definitions file: @vos - types/generated/vos.d.luau',
      "apps/x/app.luau:3:7-9: (W0) TypeError: Expected this to be exactly",
      "\t'vos_ButtonProps'",
      'caused by:',
      "  Property 'shape' is not compatible.",
      "scripts/fixtures/negative/y.luau:12:1-4: (W0) LocalUnused: Variable 'a' is never used"
    ].join('\n')
    const found = parseAnalysis(output, files)
    expect(found.get('apps/x/app.luau')?.map((f) => [f.line, f.primary])).toEqual([
      [3, "TypeError: Expected this to be exactly 'vos_ButtonProps'"]
    ])
    expect(found.get('scripts/fixtures/negative/y.luau')?.[0]?.line).toBe(12)
  })

  test('a definitions file that does not load is an error, not a clean run', () => {
    expect(() => parseAnalysis('[ERROR] cannot parse types/generated/vos.d.luau', files)).toThrow('could not use')
    expect(() => parseAnalysis('something else', files)).toThrow('unexpected luau-lsp output')
  })
})

describe('the module names an app may require', () => {
  const definitions = readFileSync(join(root, DEFINITIONS), 'utf8')
  const offered = offeredModules(definitions)

  test('are the declarations overloads less the unusable ones', () => {
    expect(offered.has('vos:app') && offered.has('vos:ui') && offered.has('lib:json') && offered.has('std:fs')).toBe(true)
    for (const refused of ['lib:test', 'lib:compile', 'std:workers']) {
      expect(offered.has(refused), refused).toBe(false)
      expect(overloads(definitions).find((o) => o.name === refused)?.type).toStartWith('vos_NotForApps_')
    }
  })

  test('a relative path, vos:app, vos:ui and an offered module are allowed; a refused or misspelt one is not', () => {
    for (const ok of ['./calc', '../shared/calc', 'vos:app', 'vos:ui', 'lib:json', 'vnd:serde_yaml']) {
      expect(allowedRequire(ok, offered), ok).toBe(true)
    }
    for (const bad of ['lib:test', 'lib:jsno', 'vos:nope', 'calc', 'json', '@pkg/x']) {
      expect(allowedRequire(bad, offered), bad).toBe(false)
    }
  })

  test('requires are read from code, not from comments', () => {
    const names = requiredNames('local a = require("lib:json") -- require("lib:test")\nlocal b = require "std:fs"\n')
    expect(names).toEqual([
      { name: 'lib:json', line: 1 },
      { name: 'std:fs', line: 2 }
    ])
  })
})

describe('the second pass (the declarations without their `declare require`)', () => {
  const definitions = readFileSync(join(root, DEFINITIONS), 'utf8')
  const offered = offeredModules(definitions)

  test('the declarations lose only the require section', () => {
    const bare = withoutRequire(definitions)
    expect(bare.split('\n').some((l) => l.startsWith('declare require'))).toBe(false)
    expect(bare).toContain('declare app: vos_AppApi')
    expect(bare).toContain('export type vos_ui = {')
    expect(definitions.startsWith(bare.trimEnd())).toBe(true)
    expect(() => withoutRequire('-- nothing\n')).toThrow('no `-- ===== require =====` section')
  })

  test('an "Unknown require" of vos:* or an offered module is dropped; of anything else it is kept', () => {
    const unknown = (name: string) => finding(3, `TypeError: Unknown require: /x/y/${name}.lua`)
    const kept = dropKnownUnknownRequires(
      [
        unknown('vos:app'),
        unknown('vos:ui'),
        unknown('lib:json'),
        unknown('lib:test'),
        unknown('lib:jsno'),
        finding(4, "TypeError: Expected this to be 'string', but got 'number'")
      ],
      offered
    )
    expect(kept.map((f) => f.primary)).toEqual([
      'TypeError: Unknown require: /x/y/lib:test.lua',
      'TypeError: Unknown require: /x/y/lib:jsno.lua',
      "TypeError: Expected this to be 'string', but got 'number'"
    ])
  })

  test('the findings of two passes are merged once each', () => {
    const merged = mergeFindings(
      [finding(5, 'TypeError: a'), finding(2, 'TypeError: b')],
      [finding(5, 'TypeError: a'), finding(5, 'TypeError: c')]
    )
    expect(merged.map((f) => [f.line, f.primary])).toEqual([
      [2, 'TypeError: b'],
      [5, 'TypeError: a'],
      [5, 'TypeError: c']
    ])
  })
})

describe('the pinned negative fixtures', () => {
  test('hold the markers NEGATIVE_MARKERS says', () => {
    for (const [file, expected] of Object.entries(NEGATIVE_MARKERS)) {
      expect(markerCount(file, readFileSync(join(root, file), 'utf8')), file).toBe(expected)
    }
  })
})

describe('which apps the repository check leaves to their own project', () => {
  const projects = new Set(['apps/calculator'])
  const isProject = (dir: string): boolean => projects.has(dir)
  test('every file of a Vite project, its sources and its generated and built Luau', () => {
    for (const file of ['apps/calculator/server/main.luau', 'apps/calculator/.app/rpc.luau', 'apps/calculator/dist/app.luau'])
      expect(inViteApp(file, isProject)).toBe(true)
  })
  test('a view-tree app, a fixture and a file named like a project elsewhere are still checked', () => {
    for (const file of ['apps/agents/app.luau', 'crates/vos-codegen/tests/fixtures/calculator/app.luau', 'scripts/fixtures/usage.luau', 'apps'])
      expect(inViteApp(file, isProject)).toBe(false)
  })
})
