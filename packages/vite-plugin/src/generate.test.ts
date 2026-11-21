import { describe, expect, test } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseDefinition } from './definition.ts'
import { emitLuau } from './emit-luau.ts'
import { emitTs } from './emit-ts.ts'
import { DEFAULTS, generate, stale } from './generate.ts'

const definition = parseDefinition(
  JSON.stringify({
    openrpc: '1.3.2',
    info: { title: 'Notes', version: '1.0.0' },
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
  const ts = emitTs(definition, 'rpc.openrpc.json')
  test('has one typed function per method, with params typed from the definition', () => {
    expect(ts).toContain("export type Note = { id: number; title: string; tag?: string | null }")
    expect(ts).toContain('list(): Promise<ListResult>')
    expect(ts).toContain('add_note(params: AddNoteParams): Promise<AddNoteResult>')
    expect(ts).toContain('export interface AddNoteParams {')
    expect(ts).toContain('title: "a" | "b"')
    expect(ts).toContain('pinned?: boolean')
    // every param optional: the params may be left out
    expect(ts).toContain('ping(params?: PingParams): Promise<PingResult>')
    expect(ts).toContain("add_note: (params) => call('add_note', params),")
  })
  test('carries the mount contract types and the JSON-RPC error class', () => {
    expect(ts).toContain('export interface MountProps')
    expect(ts).toContain('export class RpcError extends Error')
    expect(ts).toContain('export function createRpc(raw: RawRpc): Rpc')
  })
  test('is the same every time', () => expect(emitTs(definition, 'rpc.openrpc.json')).toBe(ts))
})

describe('the Luau side', () => {
  const luau = emitLuau(definition, 'rpc.openrpc.json')
  test('has the types, the handler interface the developer implements and the router', () => {
    expect(luau.startsWith('--!strict\n')).toBe(true)
    expect(luau).toContain('export type Note = { id: number, title: string, tag: string? }')
    expect(luau).toContain('export type AddNoteParams = { title: "a" | "b", pinned: boolean? }')
    expect(luau).toContain('export type Handlers = {')
    expect(luau).toContain('list: (ctx: Context) -> ListResult,')
    expect(luau).toContain('add_note: (ctx: Context, params: AddNoteParams) -> AddNoteResult,')
    expect(luau).toContain('local function router(implementation: Handlers): vos_Handlers')
    expect(luau.trimEnd().endsWith('return { router = router }')).toBe(true)
  })
  test('holds every method to its params and result', () => {
    expect(luau).toContain('add_note = {')
    expect(luau).toContain('required = { "title" }')
    expect(luau).toContain('{ kind = "ref", name = "Note" }')
  })
})

describe('the committed generated files', () => {
  const apps = resolve(import.meta.dir, '../../../e2e/apps')
  for (const name of ['tasks-svelte', 'tasks-react']) {
    test(`${name} is current with its definition`, () => {
      const project = join(apps, name)
      expect(existsSync(join(project, 'rpc.openrpc.json'))).toBe(true)
      expect(stale(project, generate(project, DEFAULTS))).toEqual([])
    })
  }
  test('the two fixtures share one definition', () => {
    const read = (name: string): string => readFileSync(join(apps, name, 'rpc.openrpc.json'), 'utf8')
    expect(read('tasks-react')).toBe(read('tasks-svelte'))
  })
})

describe('the darklua configuration', () => {
  test('is the repository\'s own', () => {
    const own = readFileSync(resolve(import.meta.dir, '../darklua.json'), 'utf8')
    const repository = readFileSync(resolve(import.meta.dir, '../../../.darklua.json'), 'utf8')
    expect(own).toBe(repository)
  })
})
