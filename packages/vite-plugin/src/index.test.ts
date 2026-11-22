/**
 * The plugin end to end: a real Vite build of a small project, through a failure at each stage. The rule under test: `dist/` is
 * only ever touched by a build that succeeded, so it is the last good build or the new complete one, and nothing is read or
 * written outside the project and its `dist/`.
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs'
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
  'index.html': '<!doctype html>\n<html lang="en"><head><title>Probe</title></head><body><div id="root"></div><script type="module" src="/src/main.ts"></script></body></html>\n',
  'src/main.ts': 'document.getElementById("root")!.textContent = "probe"\n',
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
async function run(dir: string, options: Partial<Options> = {}, warnings: string[] = []): Promise<Error | null> {
  try {
    await build({
      root: dir,
      configFile: false,
      customLogger: {
        info: () => {},
        warn: (message) => void warnings.push(message),
        warnOnce: (message) => void warnings.push(message),
        error: () => {},
        clearScreen: () => {},
        hasErrorLogged: () => false,
        hasWarned: false
      },
      plugins: [vos(options)]
    })
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

/** Whether a build left a staging folder behind in the generated folder. */
function stagingLeft(dir: string): boolean {
  return existsSync(join(dir, '.app')) && readdirSync(join(dir, '.app')).some((name) => name.startsWith('build-'))
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
    const warnings: string[] = []
    try {
      expect(await run(dir, {}, warnings)).toBeNull()
      // Vite has nothing to say about the host's classic scripts, which the plugin adds after its pass (NEOC-332)
      expect(warnings.filter((w) => w.includes("can't be bundled"))).toEqual([])
      const built = Object.keys(snapshot(join(dir, 'dist'))).sort()
      expect(built.filter((f) => !f.startsWith('static/assets/'))).toEqual(['app-ref.json', 'app.luau', 'static/index.html'])
      expect(built.some((f) => /^static\/assets\/.*\.js$/.test(f))).toBe(true)
      // the host's tags were injected first in the head: the kit's stylesheet (crossorigin), its script, then the bridge
      expect(readFileSync(join(dir, 'dist/static/index.html'), 'utf8')).toMatch(
        /<head>\s*<link rel="stylesheet" href="\/kit\/tokens\.css" crossorigin(="")?>\s*<script src="\/kit\/kit\.js"><\/script>\s*<script src="\/apps\/bridge\.js"><\/script>/
      )
      const ref = JSON.parse(readFileSync(join(dir, 'dist/app-ref.json'), 'utf8')) as { files: { path: string; sha256: string }[] }
      for (const file of ref.files) {
        expect(createHash('sha256').update(readFileSync(join(dir, 'dist', file.path))).digest('hex'), file.path).toBe(file.sha256)
      }
      expect(stagingLeft(dir)).toBe(false)
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
        ['the client does not build', { 'src/main.ts': 'const x = = 1' }, {}, /./],
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
        expect(stagingLeft(dir), `${what}: nothing left in staging`).toBe(false)
        put(dir, files()) // back to the good project for the next one
      }
      // and the next good build replaces it whole, with the new contract's migration in place
      put(dir, {
        'openrpc.json': JSON.stringify({ ...CONTRACT, info: { title: 'Probe', version: '1.1.0' }, 'x-app': { id: 'probe', migrations: [{ version: 1, name: 'init', file: 'migrations/001_init.sql' }] } }),
        'migrations/001_init.sql': 'create table t(a);'
      })
      expect(await run(dir)).toBeNull()
      const next = snapshot(join(dir, 'dist'))
      expect(Object.keys(next).filter((f) => !f.startsWith('static/assets/')).sort()).toEqual(['app-ref.json', 'app.luau', 'migrations/001_init.sql', 'static/index.html'])
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

describe.skipIf(luauLsp === null)('the host tags', () => {
  test('a page that names them itself gets each once, and the author is told to remove theirs', async () => {
    const page =
      '<!doctype html>\n<html lang="en"><head><title>Probe</title>\n  <link rel="stylesheet" href="/kit/tokens.css">\n  <script src="/kit/kit.js"></script>\n  <script src="/apps/bridge.js"></script>\n</head><body><div id="root"></div><script type="module" src="/src/main.ts"></script></body></html>\n'
    const dir = project(files({ 'index.html': page }))
    const warnings: string[] = []
    try {
      expect(await run(dir, {}, warnings)).toBeNull()
      const built = readFileSync(join(dir, 'dist/static/index.html'), 'utf8')
      for (const url of ['/kit/tokens.css', '/kit/kit.js', '/apps/bridge.js']) expect(built.split(url).length - 1, url).toBe(1)
      expect(built).toContain('href="/kit/tokens.css" crossorigin')
      expect(warnings.filter((w) => w.includes('remove yours'))).toHaveLength(1)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

describe.skipIf(luauLsp === null)('migrations and the output', () => {
  const withMigrations = (migrations: unknown[]): Record<string, string> => ({
    'openrpc.json': JSON.stringify({ ...CONTRACT, 'x-app': { id: 'probe', migrations } })
  })

  test('migrations are written under migrations/, where nothing generated can be, whatever they are called', async () => {
    const dir = project(
      files({
        ...withMigrations([
          { version: 1, name: 'page', file: 'static/index.html' },
          { version: 2, name: 'app', file: 'db/app.luau' },
          { version: 3, name: 'ref', file: 'app-ref.json' }
        ]),
        'static/index.html': 'select 1;',
        'db/app.luau': '--!strict\nreturn {}\n',
        'app-ref.json': 'select 3;'
      })
    )
    try {
      expect(await run(dir)).toBeNull()
      const built = snapshot(join(dir, 'dist'))
      expect(Object.keys(built).filter((f) => !f.startsWith('static/assets/')).sort()).toEqual(['app-ref.json', 'app.luau', 'migrations/app-ref.json', 'migrations/app.luau', 'migrations/index.html', 'static/index.html'])
      const ref = JSON.parse(readFileSync(join(dir, 'dist/app-ref.json'), 'utf8')) as { manifest: { migrations: { file: string }[] }; files: { path: string; sha256: string }[] }
      expect(ref.manifest.migrations.map((m) => m.file)).toEqual(['migrations/index.html', 'migrations/app.luau', 'migrations/app-ref.json'])
      // every path of the app ref once, with the hash of what is there
      expect(new Set(ref.files.map((f) => f.path)).size).toBe(ref.files.length)
      for (const f of ref.files) expect(built[f.path], f.path).toBe(f.sha256)
      expect(readFileSync(join(dir, 'dist/static/index.html'), 'utf8')).not.toContain('select 1')
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('duplicates and out-of-order versions are refused when the contract is read', async () => {
    const dir = project(files({ 'a.sql': 'x', 'b/a.sql': 'y' }))
    try {
      for (const [what, migrations, message] of [
        ['the same file twice', [{ version: 1, name: 'one', file: 'a.sql' }, { version: 2, name: 'two', file: 'a.sql' }], /already the file of an earlier migration/],
        ['two files of one name', [{ version: 1, name: 'one', file: 'a.sql' }, { version: 2, name: 'two', file: 'b/a.sql' }], /would be written as migrations\/a\.sql/],
        ['the same version twice', [{ version: 1, name: 'one', file: 'a.sql' }, { version: 1, name: 'two', file: 'b/a.sql' }], /must be 2/],
        ['a version that skips', [{ version: 2, name: 'one', file: 'a.sql' }], /must be 1/]
      ] as const) {
        put(dir, withMigrations([...migrations]))
        const error = await run(dir)
        expect(error?.message, what).toMatch(message)
      }
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('a link to another file of the project is that file, and one that leads out is refused naming where it leads', async () => {
    const parent = mkdtempSync(join(tmpdir(), 'vos-plugin-link-'))
    try {
      const dir = join(parent, 'app')
      mkdirSync(dir)
      put(dir, files({ 'real.sql': 'create table t(a);', ...withMigrations([{ version: 1, name: 'init', file: 'link.sql' }]) }))
      writeFileSync(join(parent, 'outside.sql'), 'drop table users;')
      symlinkSync('real.sql', join(dir, 'link.sql'))
      expect(await run(dir)).toBeNull()
      expect(readFileSync(join(dir, 'dist/migrations/link.sql'), 'utf8')).toBe('create table t(a);')
      rmSync(join(dir, 'link.sql'))
      symlinkSync(join(parent, 'outside.sql'), join(dir, 'link.sql'))
      const refused = await run(dir)
      expect(refused?.message).toMatch(/resolves to .*outside\.sql through a symbolic link, which is outside/)
      expect(refused?.message).toContain(realpathSync(parent))
    } finally {
      rmSync(parent, { recursive: true, force: true })
    }
  })
})

describe.skipIf(luauLsp === null)('builds at the same time', () => {
  test('each has a staging folder of its own, so none is lost to another and dist is always a complete build', async () => {
    const dir = project(files())
    try {
      const results = await Promise.all([run(dir), run(dir), run(dir)])
      expect(results.map((r) => r?.message ?? null)).toEqual([null, null, null])
      expect(Object.keys(snapshot(join(dir, 'dist'))).filter((f) => !f.startsWith('static/assets/')).sort()).toEqual(['app-ref.json', 'app.luau', 'static/index.html'])
      expect(stagingLeft(dir)).toBe(false)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

describe('without luau-lsp', () => {
  test('the build stops and says how to install it, unless it is turned off', async () => {
    const dir = project(files())
    const named = process.env.VOS_LUAU_LSP
    const warnings: string[] = []
    try {
      // the binary the build is told to use is not there
      process.env.VOS_LUAU_LSP = '/nonexistent/luau-lsp'
      const stopped = await run(dir)
      expect(stopped?.message).toMatch(/luau-lsp not found: install the .* release binary/)
      expect(stopped?.message).toMatch(/check: false/)
      expect(existsSync(join(dir, 'dist'))).toBe(false)
      expect(await run(dir, { check: false }, warnings)).toBeNull()
      expect(warnings.join('\n')).toContain('the check is off (check: false)')
      expect(existsSync(join(dir, 'dist/app-ref.json'))).toBe(true)
    } finally {
      if (named === undefined) delete process.env.VOS_LUAU_LSP
      else process.env.VOS_LUAU_LSP = named
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
