import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseDefinition } from './definition.ts'
import { emitLuau, emitLuauEntry } from './emit-luau.ts'
import { emitDeclarations, emitEntry, emitModule } from './emit-client.ts'
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { DEFAULTS, generate, handlerProblems, write } from './generate.ts'

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
    expect(dts).toContain('list(): Promise<ListResult>')
    expect(dts).toContain('add_note(params: AddNoteParams): Promise<AddNoteResult>')
    expect(dts).toContain('export interface AddNoteParams {')
    expect(dts).toContain('title: "a" | "b"')
    expect(dts).toContain('pinned?: boolean')
    // every param optional: the params may be left out
    expect(dts).toContain('ping(params?: PingParams): Promise<PingResult>')
    expect(dts).toContain('export const api: Api')
    expect(dts).toContain('export const host: Host')
    expect(dts).toContain('export class RpcError extends Error')
  })
  test('the module calls the host through what the entry bound', () => {
    expect(js).toContain('export function bind(props)')
    expect(js).toContain("add_note: (params) => call('add_note', params),")
    expect(js).toContain("list: () => call('list', undefined),")
    expect(js).toContain('throw new RpcError(')
  })
  test('the entry binds app:rpc to the host, then mounts the app', () => {
    const entry = emitEntry('../src/main.ts')
    expect(entry).toContain("import { mount as mountApp } from '../src/main.ts'")
    expect(entry).toContain('bind(props)\n  return mountApp(el)')
  })
  test('is the same every time', () => expect(emitDeclarations(definition, 'openrpc.json')).toBe(dts))
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
    expect(luau).toContain('local function router(implementation: Handlers): vos_Handlers')
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
      expect(files.map((f) => f.path)).toEqual(['.app/rpc.d.ts', '.app/rpc.js', '.app/entry.js', '.app/rpc.luau', '.app/entry.luau'])
      const text = readFileSync(join(project, 'openrpc.json'), 'utf8')
      const contract = parseDefinition(text)
      expect(contract.app.id).toBe(name)
      const backend = readFileSync(join(project, 'server/main.luau'), 'utf8')
      expect(handlerProblems(contract, backend)).toEqual({ missing: [], undeclared: [] })
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

describe('handlerProblems', () => {
  const contract = parseDefinition(
    JSON.stringify({
      openrpc: '1.3.2',
      info: { title: 'T', version: '1.0.0' },
      'x-app': { id: 'tasks' },
      methods: ['list', 'add'].map((name) => ({ name, params: [], result: { name: 'r', schema: { type: 'null' } } }))
    })
  )
  test('names a declared method that has no handler, and a handler that is not declared', () => {
    expect(handlerProblems(contract, 'local handlers = {\n  list = function() end,\n  other = function() end,\n}')).toEqual({
      missing: ['add'],
      undeclared: ['other']
    })
  })
  test('sees both ways of writing a handler', () => {
    expect(handlerProblems(contract, 'function app.list() end\nfunction handlers:add() end')).toEqual({ missing: [], undeclared: [] })
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
