/**
 * Where a contract's names are written into generated code, and what makes any name safe there. The table:
 *
 * | position                         | TypeScript (`rpc.d.ts`)                          | JavaScript (`rpc.js`)        | Luau (`rpc.luau`)                                         |
 * |----------------------------------|--------------------------------------------------|------------------------------|-----------------------------------------------------------|
 * | schema name (component)          | `export type N`, refs `N`: builtin and reserved  | n/a                          | `export type N`, refs: builtin types and keywords refused |
 * |                                  | type names refused; `Promise` etc. are reached   |                              |                                                           |
 * |                                  | through an alias a schema cannot shadow          |                              |                                                           |
 * | `<Method>Params`, `<Method>Result` | `export interface/type`: clashes found at read | n/a                          | `export type`: clashes found at read                      |
 * | method name                      | member of `interface Api`: quoted when a word    | `"name": ...` string key and | field of `Handlers`, key of `METHODS`: Luau keywords      |
 * |                                  | the language reads as syntax                     | string argument              | refused                                                   |
 * | param / property name            | member of an interface or type literal: quoted   | n/a                          | field of a type literal and key of a schema table: quoted |
 * |                                  | when a word                                      |                              | `["end"]` when a Luau keyword                             |
 * | enum value, title, text          | `JSON.stringify` literal / escaped comment       | escaped comment              | escaped string / comment (`escape.test.ts`)               |
 * | names that no language lets be one | `__proto__`, `constructor`, `prototype` refused everywhere; `__vos*` refused for schemas                                         |
 *
 * One table-driven test: a list of awkward names, each tried at every position. A name is either refused when the contract is read
 * (with a reason) or it is accepted, and then every generated file that mentions it is valid and means what the contract says:
 * the TypeScript compiles and has the member, the JavaScript module runs and has the key, the Luau type-checks.
 */
import { describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import ts from 'typescript'
import { DefinitionError, LUAU_KEYWORDS, parseDefinition, type Definition } from './definition.ts'
import { emitDeclarations, emitModule } from './emit-client.ts'
import { emitLuau, emitLuauEntry } from './emit-luau.ts'
import { DEFAULTS, generate, write } from './generate.ts'
import { JS_WORDS } from './names.ts'
import { analyze } from './luau-tools.ts'

const AWKWARD = [...new Set([
  // words of JavaScript, TypeScript and Luau
  ...JS_WORDS,
  ...LUAU_KEYWORDS,
  'Record', 'Array', 'Promise', 'Error', 'Date', 'Map', 'Set', 'Object', 'Function', 'String', 'Number', 'Boolean', 'Symbol', 'Partial', 'Readonly',
  'Reply', 'Host', 'Api', 'RpcError', 'Context', 'Handlers', 'Params', 'Result', 'Math', 'JSON', 'Infinity', 'NaN', 'arguments', 'eval',
  // luau builtin types and globals
  'buffer', 'thread', 'vector', 'userdata', 'table', 'math', 'string_', 'print', 'require', 'app', 'vos', 'rpc', 'error', 'pcall', 'type_', 'self',
  // the browser's and the VM's globals
  'window', 'document', 'process', 'globalThis', 'console', 'location',
  // prototype names
  '__proto__', 'constructor', 'prototype', 'toString', 'hasOwnProperty', 'valueOf', '__defineGetter__', 'isPrototypeOf',
  // generated-name shapes
  '__vos', '__vos_Reply', '__vos_x', 'COMPONENTS', 'METHODS', 'check', 'router', 'describe', 'isArray', 'allowsNull', 'provided', 'handlers',
  // differing only by case or by `_`
  'Foo', 'foo', 'FOO', 'foo_bar', 'fooBar', 'FooBar', 'Foo_Bar', 'a', 'A', '_', '_x', 'x_', 'x1', 'X1', 'ListResult', 'AddParams'
])]

const base = (over: Record<string, unknown>): Record<string, unknown> => ({
  openrpc: '1.3.2',
  info: { title: 'Names', version: '1.0.0' },
  'x-app': { id: 'names' },
  methods: [{ name: 'noop', result: { name: 'r', schema: { type: 'null' } } }],
  ...over
})
const result = { name: 'r', schema: { type: 'null' } }

/** The contract for one name at one position. */
const AT = {
  schema: (name: string) => base({ components: { schemas: { [name]: { type: 'string' } } } }),
  method: (name: string) => base({ methods: [{ name, result }] }),
  param: (name: string) => base({ methods: [{ name: 'go', params: [{ name, required: true, schema: { type: 'string' } }], result }] }),
  property: (name: string) =>
    base({
      methods: [{ name: 'go', result: { name: 'r', schema: { $ref: '#/components/schemas/Thing' } } }],
      components: { schemas: { Thing: { type: 'object', properties: { [name]: { type: 'string' } }, required: [name] } } }
    })
}

function parse(contract: Record<string, unknown>): Definition | string {
  try {
    return parseDefinition(JSON.stringify(contract))
  } catch (error) {
    if (error instanceof DefinitionError) return error.message
    throw error
  }
}

/** The names accepted at a position, each alone (a refusal has a reason). */
function accepted(position: keyof typeof AT): string[] {
  const names: string[] = []
  for (const name of AWKWARD) {
    const parsed = parse(AT[position](name))
    if (typeof parsed === 'string') expect(parsed.length, `${position} ${name}`).toBeGreaterThan(20)
    else names.push(name)
  }
  return names
}

/** One contract holding every name accepted at the position that does not clash with an earlier one. */
function together(position: 'schema' | 'method' | 'param' | 'property', names: string[]): { contract: Record<string, unknown>; used: string[] } {
  const used: string[] = []
  let contract: Record<string, unknown> = base({})
  for (const name of names) {
    const next = (() => {
      switch (position) {
        case 'schema':
          return base({ components: { schemas: Object.fromEntries([...used, name].map((n) => [n, { type: 'string' }])) } })
        case 'method':
          return base({ methods: [...used, name].map((n) => ({ name: n, params: [{ name: 'p', schema: { type: 'string' } }], result })) })
        case 'param':
          return base({ methods: [{ name: 'go', params: [...used, name].map((n) => ({ name: n, required: true, schema: { type: 'string' } })), result }] })
        case 'property':
          return base({
            methods: [{ name: 'go', result: { name: 'r', schema: { $ref: '#/components/schemas/Thing' } } }],
            components: { schemas: { Thing: { type: 'object', properties: Object.fromEntries([...used, name].map((n) => [n, { type: 'string' }])), required: [...used, name] } } }
          })
      }
    })()
    if (typeof parse(next) === 'string') continue
    contract = next
    used.push(name)
  }
  return { contract, used }
}

/** Compile `rpc.d.ts` and a file that uses it; the compiler's messages (none when it is right). */
function compile(dts: string, usage: string): string[] {
  const dir = mkdtempSync(join(tmpdir(), 'vos-names-'))
  try {
    writeFileSync(join(dir, 'rpc.d.ts'), dts)
    writeFileSync(join(dir, 'usage.ts'), usage)
    const program = ts.createProgram([join(dir, 'rpc.d.ts'), join(dir, 'usage.ts')], {
      strict: true,
      noEmit: true,
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      lib: ['lib.es2022.d.ts', 'lib.dom.d.ts'],
      types: [],
      skipLibCheck: false
    })
    return ts
      .getPreEmitDiagnostics(program)
      .map((d) => `${d.file ? d.file.fileName.split('/').pop() : ''}: ${ts.flattenDiagnosticMessageText(d.messageText, '\n')}`)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

describe('awkward names, at every position', () => {
  test('a name is refused with a reason, or accepted and then safe: TypeScript', () => {
    // schema names: each is a type that can be imported and used
    const schemas = together('schema', accepted('schema'))
    const dtsSchemas = emitDeclarations(parse(schemas.contract) as Definition, 'openrpc.json')
    const usageSchemas = [
      "import type * as R from 'app:rpc'",
      ...schemas.used.map((n, i) => `declare const s${i}: R[${JSON.stringify(n)}]\nconst t${i}: string = s${i}`)
    ].join('\n')
    // `R[...]` indexes values, not types: use a type-level lookup
    const typeUsage = [
      ...schemas.used.map((n, i) => `type T${i} = import('app:rpc').${n}`),
      ...schemas.used.map((_, i) => `declare const v${i}: T${i}\nexport const w${i}: string = v${i}`)
    ].join('\n')
    expect(usageSchemas.length).toBeGreaterThan(0)
    expect(compile(dtsSchemas, typeUsage), 'schema names').toEqual([])
    expect(schemas.used.length).toBeGreaterThan(20)

    // method names: `api[name]` exists, is callable and answers a promise
    const methods = together('method', accepted('method'))
    const dtsMethods = emitDeclarations(parse(methods.contract) as Definition, 'openrpc.json')
    const methodUsage = [
      "import { api } from 'app:rpc'",
      ...methods.used.map((n, i) => `export const m${i}: Promise<null> = api[${JSON.stringify(n)}]({ p: 'x' })`),
      ...methods.used.map((n, i) => `export const k${i}: (params?: { p?: string }) => Promise<null> = api[${JSON.stringify(n)}]`)
    ].join('\n')
    expect(compile(dtsMethods, methodUsage), 'method names').toEqual([])
    // and the interface has exactly those members, all methods (a construct signature is not one)
    const source = ts.createSourceFile('rpc.d.ts', dtsMethods, ts.ScriptTarget.ES2022, true)
    const members: string[] = []
    source.forEachChild(function walk(node) {
      if (ts.isInterfaceDeclaration(node) && node.name.text === 'Api') {
        for (const member of node.members) {
          expect(ts.isMethodSignature(member), 'a method signature').toBe(true)
          members.push(((member as ts.MethodSignature).name as ts.Identifier | ts.StringLiteral).text)
        }
      } else node.forEachChild(walk)
    })
    expect(members.sort()).toEqual([...methods.used].sort())
    expect(methods.used).toContain('new')
    expect(methods.used).toContain('delete')

    // param and property names: an object with exactly these keys type-checks
    for (const position of ['param', 'property'] as const) {
      const set = together(position, accepted(position))
      const definition = parse(set.contract) as Definition
      const dts = emitDeclarations(definition, 'openrpc.json')
      const literal = `{ ${set.used.map((n) => `${JSON.stringify(n)}: 'x'`).join(', ')} }`
      const usage =
        position === 'param'
          ? `import { api } from 'app:rpc'\nexport const call: Promise<null> = api.go(${literal})`
          : `import type { Thing } from 'app:rpc'\nexport const thing: Thing = ${literal}`
      expect(compile(dts, usage), `${position} names`).toEqual([])
      expect(set.used.length).toBeGreaterThan(20)
    }
  })

  test('JavaScript: the module runs, has one key per method, and calls the host with the method name', async () => {
    const methods = together('method', accepted('method'))
    const definition = parse(methods.contract) as Definition
    const dir = mkdtempSync(join(tmpdir(), 'vos-names-js-'))
    try {
      const file = join(dir, 'rpc.js')
      writeFileSync(file, emitModule(definition, 'openrpc.json'))
      const calls: string[] = []
      const module = (await import(`${file}?${Date.now()}`)) as {
        api: Record<string, (params?: unknown) => Promise<unknown>>
        bind(props: unknown): void
      }
      module.bind({
        rpc: async (method: string) => {
          calls.push(method)
          return null
        },
        host: { on: () => () => {}, theme: 'light' }
      })
      expect(Object.keys(module.api).sort()).toEqual([...methods.used].sort())
      for (const name of methods.used) await module.api[name]?.({ p: 'x' })
      expect(calls).toEqual(methods.used)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  const luauLsp = Bun.which('luau-lsp') ?? (existsSync(join(homedir(), '.local/bin/luau-lsp')) ? 'installed' : null)
  test.skipIf(luauLsp === null)('Luau: the generated types and tables type-check with a handler for every method', () => {
    const repository = resolve(import.meta.dir, '../../..')
    const previous = process.env.VOS_DEFINITIONS
    process.env.VOS_DEFINITIONS = join(repository, 'types/generated/vos.d.luau')
    try {
      for (const position of ['schema', 'method', 'param', 'property'] as const) {
        const set = together(position, accepted(position))
        const definition = parse(set.contract) as Definition
        const dir = mkdtempSync(join(tmpdir(), 'vos-names-luau-'))
        try {
          write(dir, [
            { path: '.app/rpc.luau', contents: emitLuau(definition, 'openrpc.json') },
            { path: '.app/entry.luau', contents: emitLuauEntry() },
            {
              path: 'server/main.luau',
              contents: [
                '--!strict',
                'local rpc = require("../.app/rpc")',
                'local handlers: rpc.Handlers = {',
                ...definition.methods.map((m) => `  ${LUAU_KEYWORDS.has(m.name) ? `["${m.name}"]` : m.name} = function(_ctx, _params: any): any return nil end,`),
                '}',
                'return handlers',
                ''
              ].join('\n')
            }
          ])
          writeFileSync(join(dir, 'openrpc.json'), JSON.stringify(set.contract))
          mkdirSync(join(dir, 'server'), { recursive: true })
          expect(analyze(dir, 'throw').findings, `${position} names`).toEqual([])
          expect(set.used.length).toBeGreaterThan(10)
        } finally {
          rmSync(dir, { recursive: true, force: true })
        }
      }
    } finally {
      if (previous === undefined) delete process.env.VOS_DEFINITIONS
      else process.env.VOS_DEFINITIONS = previous
    }
  })

  test('the names the language reserves are refused with a reason, and the ones it merely knows are accepted', () => {
    expect(parse(AT.schema('Record'))).toEqual(expect.objectContaining({ title: 'Names' }))
    expect(parse(AT.schema('Promise'))).toEqual(expect.objectContaining({ title: 'Names' }))
    expect(parse(AT.schema('Array'))).toEqual(expect.objectContaining({ title: 'Names' }))
    for (const name of ['string', 'number', 'buffer', 'thread', 'nil', 'any', 'unknown', 'class', 'end', '__vos_x', '__proto__']) {
      expect(typeof parse(AT.schema(name)), `schema ${name}`).toBe('string')
    }
    for (const name of ['end', 'function', '__proto__', 'constructor', 'prototype']) {
      expect(typeof parse(AT.method(name)), `method ${name}`).toBe('string')
    }
    for (const name of ['new', 'delete', 'type', 'class', 'default', 'typeof', 'continue']) {
      expect(typeof parse(AT.method(name)), `method ${name}`).not.toBe('string')
    }
    for (const name of ['end', 'nil', 'new', 'class', 'type']) {
      expect(typeof parse(AT.property(name)), `property ${name}`).not.toBe('string')
      expect(typeof parse(AT.param(name)), `param ${name}`).not.toBe('string')
    }
  })

  test('a pure $ref cycle is refused naming the cycle, and a cycle through an array or an object is fine', () => {
    const cyclic = (schemas: Record<string, unknown>): Record<string, unknown> => base({ components: { schemas } })
    expect(parse(cyclic({ A: { $ref: '#/components/schemas/B' }, B: { $ref: '#/components/schemas/A' } }))).toContain('A -> B -> A')
    expect(parse(cyclic({ A: { $ref: '#/components/schemas/A' } }))).toContain('A -> A')
    expect(parse(cyclic({ A: { type: ['object', 'null'], properties: {} } }))).not.toBeTypeOf('string')
    expect(parse(cyclic({ A: { $ref: '#/components/schemas/B' }, B: { type: ['string', 'null'] }, C: { $ref: '#/components/schemas/C' } }))).toContain('C -> C')
    const tree = cyclic({ Node: { type: 'object', properties: { children: { type: 'array', items: { $ref: '#/components/schemas/Node' } }, next: { $ref: '#/components/schemas/Node' } } } })
    expect(parse(tree)).not.toBeTypeOf('string')
    expect(dirname('x')).toBe('.')
  })
})
