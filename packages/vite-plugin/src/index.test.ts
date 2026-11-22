/**
 * The plugin end to end: a real Vite build of a small project, through a failure at each stage. The rule under test: `dist/` is
 * only ever touched by a build that succeeded, so it is the last good build or the new complete one, and nothing is read or
 * written outside the project and its `dist/`.
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { build } from 'vite'
import { vos, type Options } from './index.ts'

const repository = resolve(import.meta.dir, '../../..')
const luauLsp = Bun.which('luau-lsp') ?? (existsSync(join(homedir(), '.local/bin/luau-lsp')) ? 'installed' : null)

const CONTRACT = {
  openrpc: '1.3.2',
  info: { title: 'Probe', version: '1.0.0' },
  'x-app': { id: 'probe' },
  methods: [
    { name: 'list', params: [], result: { name: 'r', schema: { type: 'array', items: { type: 'string' } } } },
    { name: 'add', params: [{ name: 'title', required: true, schema: { type: 'string' } }], result: { name: 'r', schema: { type: 'null' } } }
  ]
}

const MAIN = `--!strict
local rpc = require("../.app/rpc")
local handlers: rpc.Handlers = {
  list = function(_ctx)
    return {}
  end,
  add = function(_ctx, _params)
    return nil
  end,
}
return handlers
`

const files = (extra: Record<string, string> = {}): Record<string, string> => ({
  'openrpc.json': JSON.stringify(CONTRACT),
  'server/main.luau': MAIN,
  'src/main.ts': 'export function mount(el: HTMLElement): () => void {\n  el.textContent = "probe"\n  return () => {}\n}\n',
  ...extra
})

function project(content: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), 'vos-plugin-e2e-'))
  put(dir, content)
  return dir
}

function put(dir: string, content: Record<string, string>): void {
  for (const [path, text] of Object.entries(content)) {
    mkdirSync(dirname(join(dir, path)), { recursive: true })
    writeFileSync(join(dir, path), text)
  }
}

/** The vite build of the project, with the plugin; resolves to the error it failed with, or `null`. */
async function run(dir: string, options: Partial<Options> = {}): Promise<Error | null> {
  try {
    await build({ root: dir, configFile: false, logLevel: 'silent', plugins: [vos(options)] })
    return null
  } catch (error) {
    return error instanceof Error ? error : new Error(String(error))
  }
}

/** Every file under `dir` with its hash: what a failed build must leave exactly as it was. */
function snapshot(dir: string): Record<string, string> {
  const out: Record<string, string> = {}
  const walk = (current: string, prefix: string): void => {
    if (!existsSync(current)) return
    for (const name of readdirSync(current)) {
      const path = join(current, name)
      const rel = prefix === '' ? name : `${prefix}/${name}`
      if (statSync(path).isDirectory()) walk(path, rel)
      else out[rel] = createHash('sha256').update(readFileSync(path)).digest('hex')
    }
  }
  walk(dir, '')
  return out
}

let previous: string | undefined
beforeAll(() => {
  previous = process.env.VOS_DEFINITIONS
  process.env.VOS_DEFINITIONS = join(repository, 'types/generated/vos.d.luau')
})
afterAll(() => {
  if (previous === undefined) delete process.env.VOS_DEFINITIONS
  else process.env.VOS_DEFINITIONS = previous
})

describe.skipIf(luauLsp === null)('a build through the plugin', () => {
  test('a good build is complete in dist and leaves nothing behind in the generated folder', async () => {
    const dir = project(files())
    try {
      expect(await run(dir)).toBeNull()
      expect(Object.keys(snapshot(join(dir, 'dist'))).sort()).toEqual(['app-ref.json', 'app.luau', 'client.js'])
      const ref = JSON.parse(readFileSync(join(dir, 'dist/app-ref.json'), 'utf8')) as { files: { path: string; sha256: string }[] }
      for (const file of ref.files) {
        expect(createHash('sha256').update(readFileSync(join(dir, 'dist', file.path))).digest('hex'), file.path).toBe(file.sha256)
      }
      expect(existsSync(join(dir, '.app/build'))).toBe(false)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('a failure at any stage leaves dist exactly as the last good build made it', async () => {
    const dir = project(files())
    try {
      expect(await run(dir)).toBeNull()
      const good = snapshot(join(dir, 'dist'))
      const failures: [string, Record<string, string>, Partial<Options>, RegExp][] = [
        ['the contract is not JSON', { 'openrpc.json': '{ "openrpc": ' }, {}, /not JSON at line 1/],
        ['the contract declares a method with no handler', { 'openrpc.json': JSON.stringify({ ...CONTRACT, methods: [...CONTRACT.methods, { name: 'extra', result: { name: 'r', schema: { type: 'null' } } }] }) }, {}, /does not match openrpc\.json/],
        ['the client does not build', { 'src/main.ts': 'export function mount( {' }, {}, /./],
        ['darklua cannot bundle the backend (after the client was built)', { 'server/main.luau': 'local x = = 1\n' }, { check: false }, /darklua|bundle/i],
        [
          'a migration file is missing (after the client was built)',
          { 'openrpc.json': JSON.stringify({ ...CONTRACT, 'x-app': { id: 'probe', migrations: [{ version: 1, name: 'init', file: 'migrations/001_init.sql' }] } }) },
          {},
          /migrations\[0\]\.file .* is not a file of the project/
        ]
      ]
      for (const [what, change, options, message] of failures) {
        put(dir, change)
        const error = await run(dir, options)
        expect(error, what).not.toBeNull()
        expect(`${error?.message}\n${error?.stack}`, what).toMatch(message)
        expect(snapshot(join(dir, 'dist')), `${what}: dist is untouched`).toEqual(good)
        expect(existsSync(join(dir, '.app/build')), `${what}: nothing left in staging`).toBe(false)
        put(dir, files()) // back to the good project for the next one
      }
      // and the next good build replaces it whole, with the new contract's migration in place
      put(dir, {
        'openrpc.json': JSON.stringify({ ...CONTRACT, info: { title: 'Probe', version: '1.1.0' }, 'x-app': { id: 'probe', migrations: [{ version: 1, name: 'init', file: 'migrations/001_init.sql' }] } }),
        'migrations/001_init.sql': 'create table t(a);'
      })
      expect(await run(dir)).toBeNull()
      const next = snapshot(join(dir, 'dist'))
      expect(Object.keys(next).sort()).toEqual(['app-ref.json', 'app.luau', 'client.js', 'migrations/001_init.sql'])
      expect((JSON.parse(readFileSync(join(dir, 'dist/app-ref.json'), 'utf8')) as { manifest: { version: string } }).manifest.version).toBe('1.1.0')
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('a contract cannot make a build read or write outside the project and its dist', async () => {
    const parent = mkdtempSync(join(tmpdir(), 'vos-plugin-escape-'))
    try {
      const dir = join(parent, 'app')
      mkdirSync(dir)
      put(dir, files())
      writeFileSync(join(parent, 'secret.sql'), 'drop table users;')
      for (const file of ['../secret.sql', '../../secret.sql', '/etc/hosts', 'a/../../secret.sql']) {
        put(dir, { 'openrpc.json': JSON.stringify({ ...CONTRACT, 'x-app': { id: 'probe', migrations: [{ version: 1, name: 'init', file }] } }) })
        const error = await run(dir)
        expect(error?.message, file).toMatch(/migrations\[0\]\.file/)
        expect(existsSync(join(dir, 'dist/secret.sql')), file).toBe(false)
        expect(readdirSync(parent).sort(), `${file}: nothing was written next to the project`).toEqual(['app', 'secret.sql'])
      }
    } finally {
      rmSync(parent, { recursive: true, force: true })
    }
  })
})

describe('without luau-lsp', () => {
  test('the build stops and says how to install it, unless it is turned off', async () => {
    const dir = project(files())
    const named = process.env.VOS_LUAU_LSP
    try {
      // the binary the build is told to use is not there
      process.env.VOS_LUAU_LSP = '/nonexistent/luau-lsp'
      const stopped = await run(dir)
      expect(stopped?.message).toMatch(/luau-lsp not found: install the .* release binary/)
      expect(stopped?.message).toMatch(/check: false/)
      expect(existsSync(join(dir, 'dist'))).toBe(false)
      expect(await run(dir, { check: false })).toBeNull()
      expect(existsSync(join(dir, 'dist/app-ref.json'))).toBe(true)
    } finally {
      if (named === undefined) delete process.env.VOS_LUAU_LSP
      else process.env.VOS_LUAU_LSP = named
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
