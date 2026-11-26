import { expect, test } from 'bun:test'
import { parseContract } from './contract.ts'
import { emitDependencyLuau, emitLuau } from './emit-luau.ts'
import { emitAppTypes, emitVosTypes } from './emit-ts.ts'

const contract = (methods: unknown[], schemas: Record<string, unknown> = {}) =>
  parseContract(JSON.stringify({ openrpc: '1.4.0', info: { title: 'T', version: '1.0.0' }, methods, components: { schemas } }))

const add = {
  name: 'add',
  summary: 'Add',
  params: [{ name: 'title', required: true, schema: { type: 'string', maxLength: 10 } }, { name: 'tag', schema: { type: ['string', 'null'] } }],
  result: { name: 'r', schema: { $ref: '#/components/schemas/Item' } }
}
const peek = { name: '_peek', 'x-readOnly': true, params: [], result: { name: 'r', schema: { type: 'integer' } } }
const added = { name: 'added', params: [{ name: 'item', required: true, schema: { $ref: '#/components/schemas/Item' } }] }
const saved = { name: '_saved', params: [] }
const schemas = { Item: { type: 'object', properties: { id: { type: 'integer' } }, required: ['id'] } }

test('the global vos is declared once, typed over VosApps and VosSelf, and nothing in it runs', () => {
  const d = emitVosTypes()
  expect(d).toContain('interface VosApps {}')
  expect(d).toContain('interface VosSelf {}')
  expect(d).toContain('declare const vos: {')
  for (const verb of ['sendRequest<A extends keyof VosApps', 'onRequest<M extends keyof VosRequests<VosOwnId>>', 'sendNotification<', 'onNotification<A extends keyof VosApps']) {
    expect(d).toContain(verb)
  }
  expect(d).toContain('readonly host: VosHost')
  expect(d).toContain('readonly tracing: VosTracing')
  expect(d).not.toMatch(/^(import|export) /m)
})

test("the page's own app adds every method, its types in its namespace, and says it is this page's", () => {
  const d = emitAppTypes('notes', contract([add, peek, added, saved], schemas), true)
  expect(d).toContain('declare namespace VosApp {\n  namespace Notes {')
  expect(d).toContain('type Item = { id: number }')
  expect(d).toContain('"add": { params: VosApp.Notes.AddParams; result: VosApp.Notes.AddResult }')
  expect(d).toContain('"_peek": { params: void; result: VosApp.Notes.PeekResult }')
  expect(d).toContain('"_saved": { params: void }')
  expect(d).toContain('interface VosSelf {\n  id: "notes"\n}')
  expect(d).not.toMatch(/^(import|export) /m)
})

test("an app this one depends on adds only its public methods, and is not this page's", () => {
  const d = emitAppTypes('app1', contract([add, peek, added, saved], schemas), false)
  expect(d).toContain('"app1": {')
  expect(d).toContain('"added": { params: VosApp.App1.AddedParams }')
  expect(d).not.toContain('"_peek"')
  expect(d).not.toContain('"_saved"')
  expect(d).not.toContain('VosSelf')
})

test('the backend gets modules of types only: its own app every method, a dependency its public ones', () => {
  const own = emitLuau(contract([add, peek, added, saved], schemas), 'openrpc.json', 'notes')
  expect(own).toContain('export type Item = { id: number }')
  expect(own).toContain('export type AddParams = { title: string, tag: string? }')
  expect(own).toContain('export type PeekResult = number')
  expect(own).toContain('return {}')
  expect(own).not.toContain('app.')
  const dep = emitDependencyLuau({ id: 'app1', contract: contract([add, peek, added, saved], schemas) })
  expect(dep).toContain('export type AddedParams = { item: Item }')
  expect(dep).not.toContain('Peek')
  expect(dep).not.toContain('Saved')
})
