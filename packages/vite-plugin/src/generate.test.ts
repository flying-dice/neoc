import { describe, expect, test } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { parseDefinition } from './definition.ts'
import { emitLuau, emitLuauEntry } from './emit-luau.ts'
import { emitDeclarations, emitModule } from './emit-client.ts'
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { DEFAULTS, generate, kitDeclarations, write } from './generate.ts'
import { frameworksOf } from './kit-types.ts'
import { analyze } from './luau-tools.ts'

const definition = parseDefinition(
  JSON.stringify({
    openrpc: '1.3.2',
    info: { title: 'Notes', version: '1.0.0' },
    'x-app': { id: 'notes' },
    methods: [
      { name: 'list', params: [], result: { name: 'notes', schema: { type: 'array', items: { $ref: '#/components/schemas/Note' } } } },
      {
        name: 'add_note',
        summary: 'Add a note.',
        params: [
          { name: 'title', required: true, schema: { type: 'string', enum: ['a', 'b'] } },
          { name: 'pinned', schema: { type: 'boolean' } }
        ],
        result: { name: 'note', schema: { $ref: '#/components/schemas/Note' } }
      },
      { name: 'ping', params: [{ name: 'at', schema: { type: 'number' } }], result: { name: 'ok', schema: { type: 'null' } } }
    ],
    components: {
      schemas: {
        Note: {
          type: 'object',
          properties: { id: { type: 'integer' }, title: { type: 'string' }, tag: { type: ['string', 'null'] } },
          required: ['id', 'title']
        }
      }
    }
  })
)

describe('the typed client', () => {
  const dts = emitDeclarations(definition, 'openrpc.json')
  const js = emitModule(definition, 'openrpc.json')
  test('declares the app:rpc module with one typed function per method', () => {
    expect(dts).toContain("declare module 'app:rpc' {")
    expect(dts).toContain('export type Note = { id: number; title: string; tag?: string | null }')
    expect(dts).toContain('list(): __vos_Reply<ListResult>')
    expect(dts).toContain('add_note(params: AddNoteParams): __vos_Reply<AddNoteResult>')
    expect(dts).toContain('export interface AddNoteParams {')
    expect(dts).toContain('title: "a" | "b"')
    expect(dts).toContain('pinned?: boolean')
    // every param optional: the params may be left out
    expect(dts).toContain('ping(params?: PingParams): __vos_Reply<PingResult>')
    expect(dts).toContain('export const api: Api')
    expect(dts).toContain('export const host: Host')
    expect(dts).toContain('export class RpcError extends Error')
  })
  test('the module reads the host’s bridge, which the page loads before it, and fails in words without one', () => {
    expect(js).toContain('globalThis.vos')
    expect(js).toContain('"add_note": (params) => request("add_note", params),')
    expect(js).toContain('"list": () => request("list", undefined),')
    expect(js).toContain('new RpcError(')
    expect(js).toContain('This page has no host: open the app through vos')
    expect(js).not.toContain('mount')
  })
  test('the host object offers the dialect’s services and the honest capability checks', () => {
    for (const name of ['onContext', 'onTeardown', 'openLink', 'requestDisplayMode', 'hosted', 'capabilities', 'ready'])
      expect(dts).toContain(name)
    expect(js).toContain("request('ui/open-link', { url })")
    expect(js).toContain("request('ui/request-display-mode', { mode })")
  })
  test('is the same every time', () => expect(emitDeclarations(definition, 'openrpc.json')).toBe(dts))
})

describe('null and absent are one value', () => {
  const contract = parseDefinition(
    JSON.stringify({
      openrpc: '1.3.2',
      info: { title: 'T', version: '1.0.0' },
      'x-app': { id: 'nulls' },
      methods: [
        {
          name: 'set',
          params: [{ name: 'tag', required: true, schema: { type: ['string', 'null'] } }],
          result: { name: 'r', schema: { $ref: '#/components/schemas/Thing' } }
        }
      ],
      components: {
        schemas: {
          Maybe: { type: ['string', 'null'] },
          Thing: {
            type: 'object',
            properties: { id: { type: 'integer' }, tag: { type: ['string', 'null'] }, other: { $ref: '#/components/schemas/Maybe' }, name: { type: 'string' } },
            required: ['id', 'tag', 'other', 'name']
          }
        }
      }
    })
  )
  test('a required property that allows null is optional in TypeScript, so the type matches what arrives', () => {
    const dts = emitDeclarations(contract, 'openrpc.json')
    expect(dts).toContain('export type Thing = { id: number; tag?: string | null; other?: Maybe; name: string }')
    // a required param that allows null may be left out, and so may the whole object when nothing else is required
    expect(dts).toContain('tag?: string | null')
    expect(dts).toContain('"set"(params?: SetParams): __vos_Reply<SetResult>')
  })
  test('Luau types it as nil-able and the router lets it be absent', () => {
    const luau = emitLuau(contract, 'openrpc.json')
    expect(luau).toContain('export type Thing = { id: number, tag: string?, other: Maybe, name: string }')
    expect(luau).toContain('a required property whose schema allows null may be absent')
  })
})

describe('a hostile contract cannot write code into the generated files', () => {
  const hostile = (title: string, summary: string, enumValue: string): ReturnType<typeof parseDefinition> =>
    parseDefinition(
      JSON.stringify({
        openrpc: '1.3.2',
        info: { title, version: '1.0.0' },
        'x-app': { id: 'hostile' },
        methods: [
          {
            name: 'go',
            summary,
            description: `${summary}\n*/ export const pwned = 1 /*`,
            params: [{ name: 'kind', required: true, description: summary, schema: { type: 'string', enum: [enumValue, 'plain'] } }],
            result: { name: 'r', schema: { type: 'null' } }
          }
        ],
        components: { schemas: { Note: { type: 'string', description: summary } } }
      })
    )

  const NASTY = [
    'x\nconsole.log("pwned")//',
    'x\rconsole.log("pwned")//',
    'x\r\nconsole.log("pwned")//',
    'x\u2028console.log("pwned")//',
    'x\u2029console.log("pwned")//',
    'x*/console.log("pwned")/*',
    'x]]console.log("pwned")--[[',
    'x"; console.log("pwned"); "',
    'x\\"; console.log("pwned"); //',
    'x\u0000\u001fy'
  ]

  test('title, summary, description and enum values stay comments and string literals in TypeScript and JavaScript', () => {
    for (const nasty of NASTY) {
      const contract = hostile(nasty, nasty, nasty)
      const dts = emitDeclarations(contract, 'openrpc.json')
      const js = emitModule(contract, 'openrpc.json')
      // the generated JavaScript is still one module whose only exports are the ones the generator writes
      const exports = [...new Bun.Transpiler({ loader: 'js' }).scan(js).exports]
      expect(exports.sort(), JSON.stringify(nasty)).toEqual(['RpcError', 'api', 'host'].sort())
      for (const line of js.split(/\r\n|[\r\n\u2028\u2029]/)) if (line.includes('pwned')) expect(line.trimStart().startsWith('//'), line).toBe(true)
      for (const line of dts.split(/\r\n|[\r\n\u2028\u2029]/)) {
        if (line.includes('pwned')) expect(/^\s*(\/\/|\/\*\*|\*)|^\s*kind\??: /.test(line), line).toBe(true)
      }
    }
  })

  test('and in Luau every such line is a comment or inside a string', () => {
    for (const nasty of NASTY) {
      const luau = emitLuau(hostile(nasty, nasty, nasty), 'openrpc.json')
      for (const line of luau.split('\n')) {
        if (!line.includes('pwned')) continue
        expect(/^\s*--/.test(line) || /"[^\n]*pwned[^\n]*"/.test(line), line).toBe(true)
      }
      // no raw control character or U+2028 line break escapes into a line of code
      expect(luau.replace(/\n/g, '')).not.toMatch(/[\u0000-\u001f\u007f]/)
    }
  })
})

describe('the Luau side', () => {
  const luau = emitLuau(definition, 'openrpc.json')
  test('has the types, the handler interface the developer implements and the router', () => {
    expect(luau.startsWith('--!strict\n')).toBe(true)
    expect(luau).toContain('export type Note = { id: number, title: string, tag: string? }')
    expect(luau).toContain('export type AddNoteParams = { title: "a" | "b", pinned: boolean? }')
    expect(luau).toContain('export type Handlers = {')
    expect(luau).toContain('list: (ctx: Context) -> ListResult,')
    expect(luau).toContain('add_note: (ctx: Context, params: AddNoteParams) -> AddNoteResult,')
    expect(luau).toContain('local function router(implementation: Handlers): vos_app_Handlers')
    expect(luau.trimEnd().endsWith('return { router = router }')).toBe(true)
    expect(emitLuauEntry()).toContain('return rpc.router(handlers)')
    expect(luau).toContain('error("RPC-32602 Invalid params: " .. problem, 0)')
  })
  test('holds every method to its params and result', () => {
    expect(luau).toContain('add_note = {')
    expect(luau).toContain('required = { "title" }')
    expect(luau).toContain('{ kind = "ref", name = "Note" }')
  })
})

describe('the fixtures', () => {
  const apps = resolve(import.meta.dir, '../../../e2e/apps')
  for (const name of ['tasks-svelte', 'tasks-react']) {
    test(`${name}'s contract parses and its backend has a handler for every method`, () => {
      const project = join(apps, name)
      const files = generate(project, DEFAULTS)
      const kit = name === 'tasks-svelte' ? '.app/kit-svelte.d.ts' : '.app/kit-react.d.ts'
      expect(files.map((f) => f.path)).toEqual(['.app/rpc.d.ts', '.app/rpc.js', '.app/rpc.luau', '.app/entry.luau', kit])
      const text = readFileSync(join(project, 'openrpc.json'), 'utf8')
      const contract = parseDefinition(text)
      expect(contract.app.id).toBe(name)
    })
  }
  test('the two fixtures share one contract: the same methods and schemas', () => {
    const read = (name: string): unknown => {
      const { methods, components } = JSON.parse(readFileSync(join(apps, name, 'openrpc.json'), 'utf8'))
      return { methods, components }
    }
    expect(read('tasks-react')).toEqual(read('tasks-svelte'))
  })
})

/** The handler check is the Luau type checker's: it needs luau-lsp, which the gate requires and a developer machine may lack. */
const luauLsp = Bun.which('luau-lsp') ?? (existsSync(join(homedir(), '.local/bin/luau-lsp')) ? 'installed' : null)

describe.skipIf(luauLsp === null)('the check of the backend against the contract (luau-lsp, strict)', () => {
  const repository = resolve(import.meta.dir, '../../..')
  const contract = JSON.stringify({
    openrpc: '1.3.2',
    info: { title: 'T', version: '1.0.0' },
    'x-app': { id: 'tasks' },
    methods: [
      { name: 'list', params: [], result: { name: 'r', schema: { type: 'array', items: { type: 'string' } } } },
      { name: 'add', params: [{ name: 'title', required: true, schema: { type: 'string' } }], result: { name: 'r', schema: { type: 'null' } } }
    ]
  })

  /** A project with this contract and these server files, generated, and what the check says of it. */
  function check(files: Record<string, string>): string[] {
    const dir = mkdtempSync(join(tmpdir(), 'vos-check-'))
    const previous = process.env.VOS_DEFINITIONS
    process.env.VOS_DEFINITIONS = join(repository, 'types/generated/vos.d.luau')
    try {
      writeFileSync(join(dir, 'openrpc.json'), contract)
      for (const [path, text] of Object.entries(files)) {
        mkdirSync(dirname(join(dir, path)), { recursive: true })
        writeFileSync(join(dir, path), text)
      }
      write(dir, generate(dir, DEFAULTS))
      return analyze(dir, 'throw').findings
    } finally {
      if (previous === undefined) delete process.env.VOS_DEFINITIONS
      else process.env.VOS_DEFINITIONS = previous
      rmSync(dir, { recursive: true, force: true })
    }
  }

  const header = '--!strict\nlocal rpc = require("../.app/rpc")\n'

  test('a missing handler fails, and a function the contract does not declare is never called', () => {
    const missing = check({ 'server/main.luau': `${header}local handlers: rpc.Handlers = { list = function() return {} end }\nreturn handlers\n` })
    expect(missing.join('\n')).toContain('add')
    // a function the contract does not declare is never dispatched (the router calls only declared methods), so it is no error
    const extra = check({
      'server/main.luau': `${header}local handlers: rpc.Handlers = {\n  list = function() return {} end,\n  add = function() return nil end,\n  other = function() return nil end,\n}\nreturn handlers\n`
    })
    expect(extra).toEqual([])
  })

  test('a handler of the wrong shape fails', () => {
    const wrong = check({
      'server/main.luau': `${header}local handlers: rpc.Handlers = {\n  list = function() return 5 end,\n  add = function() return nil end,\n}\nreturn handlers\n`
    })
    expect(wrong.length).toBeGreaterThan(0)
  })

  test('valid code of any shape passes: a local function, a required module, handlers built piece by piece', () => {
    expect(
      check({
        'server/main.luau': `${header}local function addFn(_ctx: rpc.Context, params: rpc.AddParams): nil\n  local _title: string = params.title\n  return nil\nend\nlocal handlers: rpc.Handlers = { list = function() return {} end, add = addFn }\nreturn handlers\n`
      })
    ).toEqual([])
    expect(
      check({
        'server/lists.luau': `--!strict\nlocal rpc = require("../.app/rpc")\nlocal function list(_ctx: rpc.Context): rpc.ListResult\n  return { "a" }\nend\nreturn list\n`,
        'server/main.luau': `${header}local list = require("./lists")\nlocal handlers: rpc.Handlers = { list = list, add = function() return nil end }\nreturn handlers\n`
      })
    ).toEqual([])
    expect(
      check({
        'server/main.luau': `${header}local function list(_ctx: rpc.Context): rpc.ListResult\n  return {}\nend\nlocal function add(_ctx: rpc.Context, _params: rpc.AddParams): rpc.AddResult\n  return nil\nend\nlocal handlers = { list = list }\nhandlers.add = add\nreturn handlers :: rpc.Handlers\n`
      })
    ).toEqual([])
  })
})

describe('write', () => {
  test('writes what changed and leaves an unchanged file alone', () => {
    const dir = mkdtempSync(join(tmpdir(), 'vos-plugin-'))
    try {
      mkdirSync(join(dir, 'x'), { recursive: true })
      const files = [{ path: 'x/a.txt', contents: 'one' }]
      expect(write(dir, files)).toEqual(['x/a.txt'])
      expect(write(dir, files)).toEqual([])
      writeFileSync(join(dir, 'x/a.txt'), 'two')
      expect(write(dir, files)).toEqual(['x/a.txt'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

describe('the darklua configuration', () => {
  test('is the repository\'s own', () => {
    const own = readFileSync(resolve(import.meta.dir, '../darklua.json'), 'utf8')
    const repository = readFileSync(resolve(import.meta.dir, '../../../.darklua.json'), 'utf8')
    expect(own).toBe(repository)
  })
})

describe("the kit's editor types", () => {
  const withPackage = (pkg: object | null): string => {
    const dir = mkdtempSync(join(tmpdir(), 'vos-kit-types-'))
    if (pkg) writeFileSync(join(dir, 'package.json'), JSON.stringify(pkg))
    return dir
  }

  test('are made for the framework the project depends on, Svelte and React alike, from either kind of dependency', () => {
    expect(frameworksOf({ devDependencies: { svelte: '^5' } })).toEqual(['svelte'])
    expect(frameworksOf({ dependencies: { react: '^19' } })).toEqual(['react'])
    expect(frameworksOf({ dependencies: { react: '^19' }, devDependencies: { svelte: '^5' } })).toEqual(['svelte', 'react'])
    expect(frameworksOf({ devDependencies: { typescript: '^5' } })).toEqual([])
    expect(frameworksOf({})).toEqual([])
  })

  test('a Svelte project gets kit-svelte.d.ts, a React one kit-react.d.ts, both gets both, a plain one none', () => {
    const cases: [object | null, string[]][] = [
      [{ devDependencies: { svelte: '^5' } }, ['.app/kit-svelte.d.ts']],
      [{ dependencies: { react: '^19' } }, ['.app/kit-react.d.ts']],
      [{ dependencies: { react: '^19', svelte: '^5' } }, ['.app/kit-svelte.d.ts', '.app/kit-react.d.ts']],
      [{ devDependencies: { typescript: '^5' } }, []],
      [null, []]
    ]
    for (const [pkg, expected] of cases) {
      const dir = withPackage(pkg)
      try {
        const files = kitDeclarations(dir, '.app')
        expect(files.map((f) => f.path), JSON.stringify(pkg)).toEqual(expected)
        for (const f of files) expect(f.contents).toContain('"ui-button"')
      } finally {
        rmSync(dir, { recursive: true, force: true })
      }
    }
  })

  test('the Svelte file augments svelte/elements and the React file React\'s JSX, from the kit\'s committed manifest', () => {
    const dir = withPackage({ dependencies: { react: '^19', svelte: '^5' } })
    try {
      const [svelte, react] = kitDeclarations(dir, '.app')
      expect(svelte!.contents).toContain("declare module 'svelte/elements'")
      expect(react!.contents).toContain("declare module 'react'")
      expect(react!.contents).not.toContain('onChange?:')
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
