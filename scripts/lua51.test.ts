/**
 * The Lua 5.1 build of the Luau apps (scripts/lua51.ts, .darklua.json, lua/prelude.luau):
 *  - always: the sources avoid what darklua cannot lower (generalized iteration) and what the prelude does not
 *    provide, every app bundles into one file whose only requires are `@agentos/*`, non-ASCII text survives as
 *    decimal byte escapes, and the polyfills agree with a native Lua 5.3+ when one is installed;
 *  - with a real Lua 5.1 (`lua5.1`, `lua-5.1`, `lua51`, or `lua` when it is 5.1; override with LUA51=<path>): the
 *    prelude, ui.lua and every app bundle load and their top levels run. Without one these tests skip with a message.
 */
import { Glob } from 'bun'
import { describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { DARKLUA_VERSION, findDarklua, root, toLua51, utf8Escapes } from './lua51'

const SKIP_DIRS = ['node_modules/', 'target/', '.claude/', '/tests/', '/e2e/', '/negative/', 'dist/']

function luauSources(patterns: string[]): string[] {
  const found: string[] = []
  for (const pattern of patterns) {
    for (const file of new Glob(pattern).scanSync({ cwd: root })) {
      if (!SKIP_DIRS.some((dir) => `/${file}`.includes(dir)) && !file.endsWith('.d.luau')) found.push(file)
    }
  }
  return found.sort()
}

const shipped = luauSources(['apps/**/*.luau', 'lua/*.luau', 'types/generated/ui.luau'])
const apps = [...new Glob('apps/*/app.json').scanSync({ cwd: root })].sort()

function entryOf(manifest: string): string {
  const main = (JSON.parse(readFileSync(join(root, manifest), 'utf8')) as { main?: string }).main ?? 'app.luau'
  return join(root, dirname(manifest), main)
}

/** A source with its `--` comments dropped (good enough to scan: a `--` inside a string only hides text). */
function code(file: string): string {
  return readFileSync(join(root, file), 'utf8')
    .split('\n')
    .map((line) => line.replace(/--.*$/, ''))
    .join('\n')
}

describe('Luau sources that must lower to Lua 5.1', () => {
  test('there are sources to check', () => {
    expect(shipped.length).toBeGreaterThan(3)
  })

  for (const file of shipped) {
    test(`${file} avoids what neither darklua nor the prelude provides`, () => {
      const source = code(file)
      // `for k, v in tbl do` (generalized iteration) has no darklua rule: write pairs(tbl) or ipairs(tbl)
      expect(source.match(/^\s*for\s+[\w, ]+\s+in\s+[\w.[\]]+\s+do\b/m)?.[0]).toBeUndefined()
      // Luau-only library members the prelude does not provide
      const unprovided =
        /\b(?:table\.(?:find|freeze|isfrozen|create|clear)|math\.(?:clamp|round|sign|noise|lerp)|string\.split|bit32\.\w+|buffer\.\w+|task\.(?:spawn|wait|defer|delay))\b/
      expect(source.match(unprovided)?.[0]).toBeUndefined()
    })
  }
})

describe('utf8Escapes', () => {
  test('writes a non-ASCII character as the decimal escapes of its UTF-8 bytes', () => {
    expect(utf8Escapes(String.raw`x = '\u{d7}\u{20ac}\u{1f600}'`)).toBe(
      String.raw`x = '\195\151\226\130\172\240\159\152\128'`
    )
  })

  test('leaves an escaped backslash before u{ alone', () => {
    expect(utf8Escapes(String.raw`'\\u{41}'`)).toBe(String.raw`'\\u{41}'`)
  })
})

describe('darklua', () => {
  const darklua = findDarklua()
  test(`is installed (pinned: ${DARKLUA_VERSION})`, () => {
    expect(darklua, `install it: cargo install darklua --version ${DARKLUA_VERSION} --locked`).toBeDefined()
  })
  if (!darklua) return

  test('strips types and lowers the Luau-only syntax', () => {
    const out = toLua51(darklua, join(root, 'types/generated/ui.luau'))
    expect(out).not.toMatch(/\bcontinue\b|`|\+=|\.\.=|::|\bexport type\b|\\u\{/)
    expect(out).not.toMatch(/:\s*(?:number|string|boolean|any)\b\s*[,)=]/)
  })

  for (const manifest of apps) {
    const dir = dirname(manifest)
    test(`${dir} bundles into one file that requires only @agentos modules`, () => {
      const out = toLua51(darklua, entryOf(manifest))
      const required = [...out.matchAll(/\brequire\s*\(\s*(['"])(.*?)\1\s*\)/g)].map((m) => m[2])
      expect(required.length).toBeGreaterThan(0)
      expect(required.filter((name) => !name?.startsWith('@agentos/'))).toEqual([])
      expect(out).not.toMatch(/\bcontinue\b|`|\+=|::|\\u\{/)
    })
  }
})

function interpreter(accept: (version: string) => boolean): string | undefined {
  const candidates = [process.env.LUA51, 'lua5.1', 'lua-5.1', 'lua51', 'lua'].filter((c): c is string => !!c)
  for (const candidate of candidates) {
    const run = spawnSync(candidate, ['-e', 'io.write(_VERSION)'], { encoding: 'utf8' })
    if (run.status === 0 && accept(run.stdout.trim())) return candidate
  }
  return undefined
}

const lua51 = interpreter((version) => version === 'Lua 5.1')
const lua53 = interpreter((version) => /^Lua 5\.([3-9])$/.test(version))
const generated = (name: string): string => join(root, 'types/generated', name)

describe('the prelude polyfills', () => {
  test.skipIf(!lua53 || !existsSync(generated('prelude.lua')))(
    'agree with the native utf8, table.move and friends of Lua 5.3+',
    () => {
      const run = spawnSync(lua53 as string, [join(root, 'scripts/lua51-polyfill-check.lua'), generated('prelude.lua')], {
        encoding: 'utf8'
      })
      expect(`${run.stdout}${run.stderr}`).toMatch(/^ok \d+ comparisons/)
      expect(run.status).toBe(0)
    }
  )
  if (!lua53) console.log('polyfill comparison SKIPPED: no Lua 5.3+ interpreter on PATH')
})

describe('under a real Lua 5.1', () => {
  if (!lua51) console.log('lua51 load tests SKIPPED: no Lua 5.1 found (install lua5.1 on PATH, or set LUA51=<path>)')
  const darklua = findDarklua()

  test.skipIf(!lua51 || !darklua)('the prelude, ui.lua and every app bundle load and run their top level', () => {
    const scratch = mkdtempSync(join(tmpdir(), 'agentos-lua51-'))
    const files = apps.map((manifest, index) => {
      const file = join(scratch, `app${index}.lua`)
      writeFileSync(file, toLua51(darklua as string, entryOf(manifest)))
      return file
    })
    const run = spawnSync(
      lua51 as string,
      [join(root, 'scripts/lua51-harness.lua'), generated('prelude.lua'), generated('ui.lua'), ...files],
      { encoding: 'utf8' }
    )
    rmSync(scratch, { recursive: true, force: true })
    expect(`${run.stdout}${run.stderr}`).toContain(`ok ${files.length}`)
    expect(run.status).toBe(0)
  })

  test.skipIf(!lua51)('every generated and bundled file parses (luac -p equivalent)', () => {
    for (const file of ['prelude.lua', 'ui.lua']) {
      const run = spawnSync(
        lua51 as string,
        ['-e', `assert(loadfile(${JSON.stringify(generated(file))}))`],
        { encoding: 'utf8' }
      )
      expect(`${file}: ${run.stderr}`).toBe(`${file}: `)
    }
  })
})
