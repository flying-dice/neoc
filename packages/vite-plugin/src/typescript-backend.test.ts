/**
 * A backend written in TypeScript, through the plugin: what is generated for it, what the compiler makes of it, and a real
 * Vite build of a small project. None of it needs luau-lsp, darklua or StyLua. That the output runs in an app's environment
 * is held where the host's runtime is (`crates/vos-lua/tests/it/typescript.rs`).
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { build } from 'vite'
import { backendOf, generate, vos } from './index.ts'
import { BackendError, compileBackend, declaredModules } from './typescript-backend.ts'

const repository = resolve(import.meta.dir, '../../..')

const CONTRACT = {
  openrpc: '1.4.0',
  info: { title: 'Probe', version: '1.0.0' },
  methods: [
    { name: 'list', summary: 'Every title.', params: [], result: { name: 'r', schema: { type: 'array', items: { $ref: '#/components/schemas/Note' } } } },
    {
      name: 'add',
      params: [
        { name: 'title', required: true, schema: { type: 'string' } },
        { name: 'pinned', schema: { type: 'boolean' } }
      ],
      result: { name: 'r', schema: { type: 'null' } }
    },
    { name: 'changed', params: [] }
  ],
  components: { schemas: { Note: { type: 'object', properties: { id: { type: 'integer' }, title: { type: 'string' } }, required: ['id', 'title'] } } }
}

const MAIN = `import { db } from 'vos:app'
import { trim } from 'lib:text'
import * as fs from 'std:fs'
import { numbered } from './notes'

RPC.on_request('list', (): probe_Note[] => numbered(db.all('select title from notes').map((row) => row.title as string)))

RPC.on_request('add', (params, ctx) => {
  if (trim(params.title) === '') return ctx.fail('A note needs a title.')
  db.exec('insert into notes(title) values (:t)', { t: params.title })
  const [text] = fs.read('seen.txt')
  app.kv.set('seen', text ?? '')
  RPC.send_notification('changed')
  return null
})
`

const NOTES = `export const numbered = (titles: string[]): probe_Note[] => titles.map((title, index) => ({ id: index + 1, title }))
`

const files = (extra: Record<string, string> = {}): Record<string, string> => ({
  'package.json': JSON.stringify({ name: '@vos-apps/probe', version: '1.0.0' }),
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
    put(dir, Object.fromEntries(generate(dir).files.map((f) => [f.path, f.contents])))
    return { bundle: compileBackend(dir, 'types'), problems: [] }
  } catch (error) {
    if (error instanceof BackendError) return { bundle: '', problems: error.problems }
    throw error
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/** A backend whose `add` handler does `body` first. */
const withBody = (body: string): string => MAIN.replace("const [text] = fs.read('seen.txt')", `${body}\n  const [text] = fs.read('seen.txt')`)

let previous: string | undefined
beforeAll(() => {
  previous = process.env.VOS_DEFINITIONS
  process.env.VOS_DEFINITIONS = join(repository, 'types/generated/vos.d.luau')
})
afterAll(() => {
  if (previous === undefined) delete process.env.VOS_DEFINITIONS
  else process.env.VOS_DEFINITIONS = previous
})

describe('a backend in TypeScript', () => {
  test('it is server/main.ts or server/main.luau, and not both', () => {
    const ts = project({ 'server/main.ts': '' })
    const luau = project({ 'server/main.luau': '' })
    const both = project({ 'server/main.ts': '', 'server/main.luau': '' })
    const none = project({})
    try {
      expect(backendOf(ts)).toEqual({ language: 'typescript', path: 'server/main.ts' })
      expect(backendOf(luau)).toEqual({ language: 'luau', path: 'server/main.luau' })
      expect(() => backendOf(both)).toThrow('server/main.luau and server/main.ts are both there')
      expect(() => backendOf(none)).toThrow('not found')
    } finally {
      for (const dir of [ts, luau, both, none]) rmSync(dir, { recursive: true, force: true })
    }
  })

  test('it gets types for TypeScript and not Luau: the declarations and the config that checks it', () => {
    const dir = project(files())
    try {
      const paths = generate(dir).files.map((f) => f.path)
      expect(paths).toEqual(expect.arrayContaining(['types/vos.d.ts', 'types/probe.d.ts', 'types/vos.backend.d.ts', 'types/tsconfig.backend.json']))
      expect(paths).not.toContain('types/vos.d.luau')
      const config = JSON.parse(generate(dir).files.find((f) => f.path === 'types/tsconfig.backend.json')?.contents ?? '') as { include: string[]; compilerOptions: { types: string[] } }
      expect(config.include).toEqual(['../server/**/*.ts', './vos.backend.d.ts'])
      expect(config.compilerOptions.types).toEqual([])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('the modules the declarations declare are the ones the host answers', () => {
    const modules = declaredModules(readFileSync(join(repository, 'types/generated/vos.d.ts'), 'utf8'))
    expect(modules).toEqual(expect.arrayContaining(['vos:app', 'vos:rpc', 'std:fs', 'lib:text']))
  })

  test('one Luau chunk: the modules bundled, the host’s modules required, the bus called through the global', () => {
    const { bundle, problems } = compile(MAIN)
    expect(problems).toEqual([])
    expect(bundle).toContain('require("vos:app")')
    expect(bundle).toContain('require("lib:text")')
    const flat = bundle.replace(/\s+/g, ' ')
    expect(flat).toContain('RPC.on_request( "list",')
    expect(flat).toContain('RPC.send_notification("changed")')
    expect(bundle).not.toContain('require("./notes")')
  })

  test('a call that does not match the contract does not compile', () => {
    const wrongMethod = compile(MAIN.replace("RPC.send_notification('changed')", "RPC.send_notification('chnaged')"))
    expect(wrongMethod.problems).toEqual([expect.stringMatching(/^server\/main\.ts:\d+:\d+: Argument of type '"chnaged"' is not assignable/)])
    const wrongHandler = compile(MAIN.replace('(): probe_Note[] =>', '(): string[] =>').replace('numbered(', '('))
    expect(wrongHandler.problems).not.toEqual([])
    const wrongApp = compile(withBody("RPC.send_request('other', 'add', { title: 'x' })"))
    expect(wrongApp.problems).not.toEqual([])
  })

  test('what an app is not offered does not compile', () => {
    expect(compile(withBody("const tick = Bun.nanoseconds()\n  app.kv.set('t', tick)")).problems).not.toEqual([])
  })

  test('the bitwise operators are Luau’s bit32, and a compound one is refused in words', () => {
    const { bundle, problems } = compile(withBody('const bits = [6 & 3, 6 | 1, 6 ^ 3, 1 << 4, 256 >> 2, 256 >>> 4, ~7]\n  app.kv.set("bits", bits)'))
    expect(problems).toEqual([])
    expect(bundle.replace(/\s+/g, ' ')).toContain(
      'local bits = { bit32.band(6, 3), bit32.bor(6, 1), bit32.bxor(6, 3), bit32.lshift(1, 4), bit32.arshift(256, 2), bit32.rshift(256, 4), bit32.bnot(7) }'
    )
    const compound = compile(withBody('let mask = 6\n  mask &= 3\n  app.kv.set("mask", mask)'))
    expect(compound.problems).toEqual([expect.stringMatching(/^server\/main\.ts:\d+:\d+: `&=` is not compiled for Luau: write the assignment out \(`x = x & y`\)$/)])
  })

  test('a global of TypeScript’s library that an app does not have is refused, and a name of the developer’s own is not', () => {
    const { problems } = compile(withBody('app.kv.set("now", Date.now())\n  app.kv.set("json", JSON.stringify(params))'))
    expect(problems).toEqual([
      expect.stringMatching(/^server\/main\.ts:\d+:\d+: `Date` is not in an app's environment: use `std:time` or `lib:chrono`$/),
      expect.stringMatching(/^server\/main\.ts:\d+:\d+: `JSON` is not in an app's environment: use `lib:json`$/)
    ])
    const own = compile(withBody('const JSON = { stringify: (value: unknown): string => String(value) }\n  app.kv.set("json", JSON.stringify(params))'))
    expect(own.problems).toEqual([])
  })

  test('a test beside the backend is not part of the app', () => {
    const { bundle, problems } = compile(MAIN, { 'server/notes.test.ts': "import { test } from 'bun:test'\ntest('x', () => {})\n" })
    expect(problems).toEqual([])
    expect(bundle).not.toContain('bun:test')
  })

  const run = async (dir: string): Promise<Error | null> => {
    const quiet = { info: () => {}, warn: () => {}, warnOnce: () => {}, error: () => {}, clearScreen: () => {}, hasErrorLogged: () => false, hasWarned: false }
    try {
      await build({ root: dir, configFile: false, customLogger: quiet, plugins: [vos()] })
      return null
    } catch (error) {
      return error instanceof Error ? error : new Error(String(error))
    }
  }

  test('a good build is one installable app, and a backend that does not compile fails the build', async () => {
    const dir = project(files())
    try {
      expect(await run(dir)).toBeNull()
      expect(readFileSync(join(dir, 'dist/app.luau'), 'utf8')).toContain('"add"')
      const ref = JSON.parse(readFileSync(join(dir, 'dist/app-ref.json'), 'utf8')) as { manifest: { main: string }; files: { path: string }[] }
      expect(ref.manifest.main).toBe('app.luau')
      expect(ref.files.map((f) => f.path)).toEqual(expect.arrayContaining(['app.luau', 'openrpc.json', 'static/index.html']))

      put(dir, { 'server/main.ts': MAIN.replace("RPC.on_request('list'", "RPC.on_request('lsit'") })
      const failed = await run(dir)
      expect(failed?.message).toMatch(/server\/main\.ts does not compile \(TypeScript, to Luau\):\nserver\/main\.ts:\d+:\d+: /)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
