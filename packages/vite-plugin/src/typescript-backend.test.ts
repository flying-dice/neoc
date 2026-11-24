/**
 * A backend written in TypeScript, through the plugin: what is generated for it, what the compiler makes of it, and a real
 * Vite build of a small project. None of it needs luau-lsp, darklua or StyLua. That the output runs in an app's environment
 * is held where the host's runtime is (`crates/vos-lua/tests/it/typescript.rs`, `todos.rs`).
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { build } from 'vite'
import { backendBundle } from './backend.ts'
import { parseDefinition } from './definition.ts'
import { emitLuau } from './emit-luau.ts'
import { DEFAULTS, generate, removeOtherBackend, resolveOptions, write } from './generate.ts'
import { vos } from './index.ts'
import { BackendError, declaredModules, emitServerEntry, emitServerRouter, emitServerTypes } from './typescript-backend.ts'

const repository = resolve(import.meta.dir, '../../..')

const CONTRACT = {
  openrpc: '1.3.2',
  info: { title: 'Probe', version: '1.0.0' },
  'x-app': { id: 'probe' },
  methods: [
    { name: 'list', summary: 'Every title.', params: [], result: { name: 'r', schema: { type: 'array', items: { $ref: '#/components/schemas/Note' } } } },
    {
      name: 'add',
      params: [
        { name: 'title', required: true, schema: { type: 'string' } },
        { name: 'pinned', schema: { type: 'boolean' } }
      ],
      result: { name: 'r', schema: { type: 'null' } }
    }
  ],
  components: { schemas: { Note: { type: 'object', properties: { id: { type: 'integer' }, title: { type: 'string' } }, required: ['id', 'title'] } } }
}

const MAIN = `import { db, tool, t } from 'vos:app'
import { trim } from 'lib:text'
import * as fs from 'std:fs'
import type { Handlers, Note } from '../.app/server/rpc'
import { numbered } from './notes'

const handlers: Handlers = {
  list: (): Note[] => numbered(db.all('select title from notes').map((row) => row.title as string)),
  add: (ctx, params) => {
    if (trim(params.title) === '') return ctx.fail('A note needs a title.')
    db.exec('insert into notes(title) values (:t)', { t: params.title })
    const [text] = fs.read('seen.txt')
    app.kv.set('seen', text ?? '')
    return null
  }
}

tool({ name: 'count', input: { folder: t.string({ optional: true }) }, run: () => ({ n: handlers.list(undefined as never).length }) })

export default handlers
`

const NOTES = `import type { Note } from '../.app/server/rpc'

export const numbered = (titles: string[]): Note[] => titles.map((title, index) => ({ id: index + 1, title }))
`

const files = (extra: Record<string, string> = {}): Record<string, string> => ({
  'openrpc.json': JSON.stringify(CONTRACT),
  'server/main.ts': MAIN,
  'server/notes.ts': NOTES,
  'index.html': '<!doctype html>\n<html lang="en"><head><title>Probe</title></head><body><div id="root"></div><script type="module" src="/src/main.ts"></script></body></html>\n',
  'src/main.ts': 'document.getElementById("root")!.textContent = "probe"\n',
  ...extra
})

function put(dir: string, content: Record<string, string>): void {
  for (const [path, text] of Object.entries(content)) {
    mkdirSync(dirname(join(dir, path)), { recursive: true })
    writeFileSync(join(dir, path), text)
  }
}

function project(content: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), 'vos-plugin-ts-'))
  put(dir, content)
  return dir
}

/** The backend of a project with `main` as its `server/main.ts`, compiled; or the problems the compiler found. */
function compile(main: string, extra: Record<string, string> = {}): { bundle: string; problems: string[] } {
  const dir = project(files({ 'server/main.ts': main, ...extra }))
  try {
    const settings = resolveOptions(dir)
    write(dir, generate(dir, settings))
    return { bundle: backendBundle(dir, settings), problems: [] }
  } catch (error) {
    if (error instanceof BackendError) return { bundle: '', problems: error.problems }
    throw error
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/** A backend whose one handler does `body`. */
const withBody = (body: string): string => MAIN.replace("const [text] = fs.read('seen.txt')", `${body}\n    const [text] = fs.read('seen.txt')`)

let previous: string | undefined
beforeAll(() => {
  previous = process.env.VOS_DEFINITIONS
  process.env.VOS_DEFINITIONS = join(repository, 'types/generated/vos.d.luau')
})
afterAll(() => {
  if (previous === undefined) delete process.env.VOS_DEFINITIONS
  else process.env.VOS_DEFINITIONS = previous
})

describe('what is generated for a backend in TypeScript', () => {
  const definition = parseDefinition(JSON.stringify(CONTRACT), 'openrpc.json')

  test('the handlers are typed from the definition, as plain functions', () => {
    const types = emitServerTypes(definition, 'openrpc.json')
    expect(types).toContain('export type Context = vos_app_Context')
    expect(types).toContain('export type Note = { id: number; title: string }')
    expect(types).toContain('export interface AddParams {\n  title: string\n  pinned?: boolean\n}')
    expect(types).toContain('  /** Every title. */\n  list(this: void, ctx: Context): ListResult')
    expect(types).toContain('  add(this: void, ctx: Context, params: AddParams): AddResult')
    expect(types).toContain('export function router(this: void, handlers: Handlers): vos_app_Handlers')
  })

  test('the router is the one a Luau backend gets', () => {
    const router = emitServerRouter(definition, 'openrpc.json')
    const luau = emitLuau(definition, 'openrpc.json')
    expect(luau.endsWith(router.slice(router.indexOf('local COMPONENTS')))).toBe(true)
    expect(router).toContain('return { router = router }')
  })

  test('the entry puts the handlers the backend exports by default behind the router', () => {
    // (the paths are put together here so that the repository's own import check does not read them as this file's)
    const from = (path: string): string => `import handlers from ${JSON.stringify(`../../${path}`)}\n`
    expect(emitServerEntry('.app', 'server/main.ts')).toContain(`${from('server/main')}import { router } from './rpc'\n\nexport = router(handlers)\n`)
    expect(emitServerEntry('.app', 'backend/src/index.ts')).toContain(from('backend/src/index'))
  })

  test('a project with server/main.ts and no server/main.luau is a TypeScript one, unless it says otherwise', () => {
    const dir = project(files())
    try {
      expect(resolveOptions(dir).backend).toBe('server/main.ts')
      expect(resolveOptions(dir, { backend: 'server/other.luau' }).backend).toBe('server/other.luau')
      put(dir, { 'server/main.luau': '--!strict\nreturn {}\n' })
      expect(resolveOptions(dir).backend).toBe('server/main.luau')
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('its files replace the Luau ones, and the other language’s are removed', () => {
    const dir = project(files())
    try {
      const luau = { ...DEFAULTS, check: false }
      put(dir, { 'server/main.luau': '--!strict\nreturn {}\n' })
      write(dir, generate(dir, luau))
      expect(existsSync(join(dir, '.app/entry.luau'))).toBe(true)

      const settings = resolveOptions(dir, { backend: 'server/main.ts' })
      const written = write(dir, generate(dir, settings)).sort()
      expect(written).toEqual(['.app/server/entry.ts', '.app/server/rpc.d.ts', '.app/server/rpc.lua', '.app/server/tsconfig.json', '.app/server/vos.d.ts'])
      expect(removeOtherBackend(dir, settings)).toEqual(['.app/rpc.luau', '.app/entry.luau'])
      expect(existsSync(join(dir, '.app/entry.luau'))).toBe(false)
      // the host's declarations are a copy of the generated ones
      expect(readFileSync(join(dir, '.app/server/vos.d.ts'), 'utf8')).toBe(readFileSync(join(repository, 'types/generated/vos.d.ts'), 'utf8'))

      // and back
      write(dir, generate(dir, luau))
      expect(removeOtherBackend(dir, luau)).toEqual(['.app/server'])
      expect(existsSync(join(dir, '.app/server'))).toBe(false)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('the modules the declarations declare are the ones the host answers', () => {
    const modules = declaredModules(readFileSync(join(repository, 'types/generated/vos.d.ts'), 'utf8'))
    expect(modules).toContain('vos:app')
    expect(modules).toContain('std:fs')
    expect(modules).not.toContain('lib:test')
  })
})

describe('compiling a backend in TypeScript', () => {
  test('one Luau chunk: the modules bundled, the host’s modules required, methods called with `:` and functions with `.`', () => {
    const { bundle, problems } = compile(MAIN)
    expect(problems).toEqual([])
    // the host's modules stay runtime requires, the app's own are in the chunk
    expect(bundle).toContain('require("vos:app")')
    expect(bundle).toContain('require("lib:text")')
    expect(bundle).toContain('["server.notes"] = function(...)')
    expect(bundle).toContain('["server.main"] = function(...)')
    // a method of a value, a function of a module, several values back, the global
    expect(bundle).toContain('db:all("select title from notes")')
    expect(bundle).toContain('trim(params.title) == ""')
    expect(bundle).toContain('ctx:fail("A note needs a title.")')
    expect(bundle).toContain('local text = fs.read("seen.txt")')
    expect(bundle).toContain('app.kv:set(')
    // a handler is a plain function, as the router calls it
    expect(bundle).toContain('add = function(ctx, params)')
    // the entry returns the handlers behind the router, which is in the chunk
    expect(bundle).toContain('local ____exports = router(handlers)')
    expect(bundle).toContain('local function router(implementation: Handlers): vos_app_Handlers')
    // an app's environment has no `debug`: the chunk brings what the compiler's Error asks of it
    expect(bundle.startsWith('-- An app\'s environment has no `debug` library')).toBe(true)
  })

  test('a handler that is missing, or of the wrong shape, does not compile', () => {
    const missing = compile(MAIN.replace(/  add: \(ctx, params\) => \{[\s\S]*?\n  \}\n\}/, '}'))
    expect(missing.problems.join('\n')).toMatch(/server\/main\.ts:\d+:\d+: Property 'add' is missing/)

    const wrong = compile(MAIN.replace('(): Note[] => numbered(', '(): string[] => ('))
    expect(wrong.problems.join('\n')).toMatch(/server\/main\.ts:\d+:\d+: .*not assignable/)

    const untyped = compile("export default { list: () => [{ id: '1', title: 'a' }], add: () => null }\n")
    expect(untyped.problems.join('\n')).toMatch(/\.app\/server\/entry\.ts:\d+:\d+: .*not assignable/)

    const none = compile('export const handlers = {}\n')
    expect(none.problems.join('\n')).toMatch(/\.app\/server\/entry\.ts:\d+:\d+: .*no default export/)
  })

  test('what an app is not offered does not compile', () => {
    for (const [line, said] of [
      ["import * as refused from 'lib:test'\nrefused", /Cannot find module 'lib:test'/],
      ["print('x')", /Cannot find name 'print'/],
      ["console.log('x')", /Cannot find name 'console'/],
      ["document.title = 'x'", /Cannot find name 'document'/]
    ] as const) {
      expect(compile(`${line}\n${MAIN}`).problems.join('\n'), line).toMatch(said)
    }
  })

  test('the bitwise operators are Luau’s bit32, and a compound one is refused in words', () => {
    const { bundle, problems } = compile(withBody('const bits = [6 & 3, 6 | 1, 6 ^ 3, 1 << 4, 256 >> 2, 256 >>> 4, ~7]\n    app.kv.set("bits", bits)'))
    expect(problems).toEqual([])
    expect(bundle.replace(/\s+/g, ' ')).toContain(
      'local bits = { bit32.band(6, 3), bit32.bor(6, 1), bit32.bxor(6, 3), bit32.lshift(1, 4), bit32.arshift(256, 2), bit32.rshift(256, 4), bit32.bnot(7) }'
    )
    const compound = compile(withBody('let mask = 6\n    mask &= 3\n    app.kv.set("mask", mask)'))
    expect(compound.problems).toEqual([expect.stringMatching(/^server\/main\.ts:\d+:\d+: `&=` is not compiled for Luau: write the assignment out \(`x = x & y`\)$/)])
  })

  test('a global of TypeScript’s library that an app does not have is refused, and a name of the developer’s own is not', () => {
    const { problems } = compile(withBody('app.kv.set("now", Date.now())\n    app.kv.set("json", JSON.stringify(params))'))
    expect(problems).toEqual([
      expect.stringMatching(/^server\/main\.ts:\d+:\d+: `Date` is not in an app's environment: use `std:time` or `lib:chrono`$/),
      expect.stringMatching(/^server\/main\.ts:\d+:\d+: `JSON` is not in an app's environment: use `lib:json`$/)
    ])
    expect(compile(withBody('app.kv.set("globals", { Date })')).problems).toEqual([expect.stringMatching(/`Date` is not in an app's environment/)])
    const own = compile(withBody('const JSON = { stringify: (value: unknown): string => String(value) }\n    app.kv.set("json", JSON.stringify(params))'))
    expect(own.problems).toEqual([])
  })

  test('a test beside the backend is not part of the app', () => {
    const { bundle, problems } = compile(MAIN, { 'server/notes.test.ts': "import { test } from 'bun:test'\ntest('x', () => {})\n" })
    expect(problems).toEqual([])
    expect(bundle).not.toContain('bun:test')
  })

  test('what TypeScriptToLua does not compile is a problem of the build, with its place', () => {
    const { problems } = compile(withBody('const day = /^\\d+$/.test(params.title)\n    app.kv.set("day", day)'))
    expect(problems.join('\n')).toMatch(/server\/main\.ts:\d+:\d+: /)
  })
})

describe('a build of a project whose backend is TypeScript', () => {
  const run = async (dir: string): Promise<Error | null> => {
    const quiet = { info: () => {}, warn: () => {}, warnOnce: () => {}, error: () => {}, clearScreen: () => {}, hasErrorLogged: () => false, hasWarned: false }
    try {
      await build({ root: dir, configFile: false, customLogger: quiet, plugins: [vos()] })
      return null
    } catch (error) {
      return error instanceof Error ? error : new Error(String(error))
    }
  }

  test('a good build is one installable app, and a backend that does not compile leaves it as it was', async () => {
    const dir = project(files())
    try {
      expect(await run(dir)).toBeNull()
      const built = readFileSync(join(dir, 'dist/app.luau'), 'utf8')
      expect(built).toContain('local ____exports = router(handlers)')
      const ref = JSON.parse(readFileSync(join(dir, 'dist/app-ref.json'), 'utf8')) as { manifest: { main: string }; files: { path: string }[] }
      expect(ref.manifest.main).toBe('app.luau')
      expect(ref.files.map((f) => f.path)).toContain('static/index.html')

      put(dir, { 'server/main.ts': MAIN.replace('(): Note[] => numbered(', '(): string[] => (') })
      const failed = await run(dir)
      expect(failed?.message).toMatch(/server\/main\.ts does not compile \(TypeScript, to Luau\):\nserver\/main\.ts:\d+:\d+: /)
      expect(readFileSync(join(dir, 'dist/app.luau'), 'utf8')).toBe(built)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('a build that stops on the backend still watches it, so that the fix is built', async () => {
    const dir = project(files({ 'server/main.ts': MAIN.replace('(): Note[] => numbered(', '(): string[] => (') }))
    const watched: string[] = []
    const warned: string[] = []
    const plugin = vos()
    const start = plugin.buildStart as (this: { addWatchFile: (file: string) => void; warn: (message: string) => void }) => void
    try {
      ;(plugin.config as (config: { root: string }) => unknown)({ root: dir })
      expect(() => start.call({ addWatchFile: (file) => watched.push(file), warn: (message) => warned.push(message) })).toThrow(/does not compile/)
      expect(watched).toEqual(expect.arrayContaining([join(dir, 'openrpc.json'), join(dir, 'server/main.ts'), join(dir, 'server/notes.ts')]))
      expect(warned).toEqual([])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
