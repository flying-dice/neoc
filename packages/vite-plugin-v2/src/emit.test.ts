import { expect, test } from 'bun:test'
import { parseContract } from './contract.ts'
import { emitLuauDefinitions } from './emit-luau.ts'
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

test('the globals RPC and vos are declared once, typed over VosApps and VosSelf, and nothing in them runs', () => {
  const d = emitVosTypes()
  expect(d).toContain('interface VosApps {}')
  expect(d).toContain('interface VosSelf {}')
  expect(d).toContain('declare const RPC: {')
  expect(d).toContain('declare const vos: {')
  expect(d).not.toMatch(/sendRequest|onRequest|sendNotification|onNotification/)
  for (const verb of ['send_request<A extends keyof VosApps', 'on_request<M extends keyof VosRequests<VosOwnId>>', 'send_notification<', 'on_notification<A extends keyof VosApps', 'about?: VosAbout']) {
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

/** The lines of the host's declarations the generator types: the global `app`'s bus functions, as the host writes them. */
const HOST = `export type vos_rpc = {
	send_request: (app: string, method: string, params: { [string]: any }?) -> (any, vos_rpc_RequestError?),
	on_request: (method: string, handler: (arg1: any, arg2: vos_app_Context) -> any, about: vos_rpc_About?) -> (),
	send_notification: (name: string, params: any?) -> (),
	on_notification: (app: string, name: string, listener: (arg1: any, arg2: vos_app_Context) -> ()) -> (),
}
declare RPC: vos_rpc
`

test("the backend's globals are typed: the host's declarations with each app's types and RPC's functions overloaded", () => {
  const d = emitLuauDefinitions(HOST, 'notes', contract([add, peek, added, saved], schemas), [
    { id: 'app-one', contract: contract([add, peek, added, saved], schemas) }
  ])
  // each app's types are global, under its id; a dependency's private ones are left out
  expect(d).toContain('export type notes_Item = { id: number }')
  expect(d).toContain('export type notes_AddParams = { title: string, tag: string? }')
  expect(d).toContain('export type notes_PeekResult = number')
  expect(d).toContain('export type app_one_AddResult = app_one_Item')
  expect(d).not.toContain('app_one_PeekResult')
  // the bus functions, by app and method
  expect(d).toContain('send_request: ((app: "notes", method: "add", params: notes_AddParams) -> (notes_AddResult?, vos_rpc_RequestError?)) &')
  expect(d).toContain('((app: "notes", method: "_peek", params: {}?) -> (notes_PeekResult?, vos_rpc_RequestError?))')
  expect(d).toContain('((app: "app-one", method: "add", params: app_one_AddParams) -> (app_one_AddResult?, vos_rpc_RequestError?))')
  expect(d).not.toContain('method: "_peek", params: {}?) -> (app_one')
  expect(d).toContain('on_request: ((method: "add", handler: (params: notes_AddParams, ctx: vos_app_Context) -> notes_AddResult, about: vos_rpc_About?) -> ())')
  expect(d).toContain('send_notification: ((name: "added", params: notes_AddedParams) -> ()) & ((name: "_saved", params: {}?) -> ()),')
  expect(d).toContain('((app: "app-one", name: "added", listener: (params: app_one_AddedParams, ctx: vos_app_Context) -> ()) -> ())')
  expect(d).toContain('declare RPC: vos_rpc')
  expect(d).not.toContain('require(')
})

test('declarations without the bus functions are refused with a message', () => {
  expect(() => emitLuauDefinitions('declare RPC: {}', 'notes', contract([add], schemas), [])).toThrow('older than this plugin')
})
