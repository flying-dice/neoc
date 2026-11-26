import { expect, test } from 'bun:test'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import ts from 'typescript'
import { parseContract } from './contract.ts'
import { emitLuauDefinitions } from './emit-luau.ts'
import { emitBackendTsconfig, emitBackendTypes } from './emit-backend-ts.ts'
import { emitAppTypes, emitVosTypes } from './emit-ts.ts'
import { writeKitTypes } from './index.ts'

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

/** The lines of the host's TypeScript declarations the generator types: `vos:rpc`'s functions, as the host writes them. */
const HOST_TS = `type LuaMultiReturn<T extends unknown[]> = T
interface vos_app_Context { readonly actor: string; fail(message: string): never }
interface vos_rpc_About { title?: string }
type vos_rpc_RequestError = { kind: "not_found" | "failed" | "internal"; message: string }
declare module "vos:rpc" {
  /** Send a request. */
  export function send_request(this: void, app: string, method: string, params?: { [key: string]: unknown }): LuaMultiReturn<[unknown, undefined] | [undefined, vos_rpc_RequestError]>
  export function on_request(this: void, method: string, handler: (this: void, arg1: unknown, arg2: vos_app_Context) => unknown, about?: vos_rpc_About): void
  export function send_notification(this: void, name: string, params?: unknown): void
  export function on_notification(this: void, app: string, name: string, listener: (this: void, arg1: unknown, arg2: vos_app_Context) => void): void
}
declare const RPC: typeof import("vos:rpc")
`

const notes = contract([add, peek, added, saved], schemas)
const dependency = { id: 'app-one', contract: contract([add, peek, added, saved], schemas) }

test("a TypeScript backend's globals are typed: the host's declarations with each app's types and vos:rpc's functions overloaded", () => {
  const d = emitBackendTypes(HOST_TS, 'notes', notes, [dependency])
  // each app's types are global, under its id; a dependency's private ones are left out
  expect(d).toContain('type notes_Item = { id: number }')
  expect(d).toContain('interface notes_AddParams {\n  title: string\n  tag?: string | null\n}')
  expect(d).toContain('type notes_PeekResult = number')
  expect(d).toContain('type app_one_AddResult = app_one_Item')
  expect(d).not.toContain('app_one_PeekResult')
  // the functions, by app and method
  expect(d).toContain('export function send_request(this: void, app: "notes", method: "add", params: notes_AddParams): LuaMultiReturn<[notes_AddResult, undefined] | [undefined, vos_rpc_RequestError]>')
  expect(d).toContain('export function send_request(this: void, app: "notes", method: "_peek", params?: { [key: string]: never }): LuaMultiReturn<[notes_PeekResult, undefined]')
  expect(d).toContain('export function send_request(this: void, app: "app-one", method: "add", params: app_one_AddParams)')
  expect(d).not.toContain('app: "app-one", method: "_peek"')
  expect(d).toContain('export function on_request(this: void, method: "add", handler: (this: void, params: notes_AddParams, ctx: vos_app_Context) => notes_AddResult, about?: vos_rpc_About): void')
  expect(d).toContain('export function send_notification(this: void, name: "added", params: notes_AddedParams): void')
  expect(d).toContain('export function send_notification(this: void, name: "_saved"): void')
  expect(d).toContain('export function on_notification(this: void, app: "app-one", name: "added", listener: (this: void, params: app_one_AddedParams, ctx: vos_app_Context) => void): void')
  expect(d).not.toContain('app: "app-one", name: "_saved"')
  // the host's own, untyped, are gone
  expect(d).not.toContain('method: string')
  expect(d).toContain('declare const RPC: typeof import("vos:rpc")')
})

test('declarations without the bus functions are refused with a message, for TypeScript too', () => {
  expect(() => emitBackendTypes('declare const RPC: {}', 'notes', contract([add], schemas), [])).toThrow('older than this plugin')
})

test('the backend is checked from its own config: its folder, the declarations, and none of the page', () => {
  const config = JSON.parse(emitBackendTsconfig('/types-root', '..')) as { compilerOptions: Record<string, unknown>; include: string[]; exclude: string[] }
  expect(config.include).toEqual(['../server/**/*.ts', './vos.backend.d.ts'])
  expect(config.exclude).toEqual(['../server/**/*.test.ts', '../server/**/*.spec.ts'])
  expect(config.compilerOptions).toMatchObject({ types: [], typeRoots: ['/types-root'], strict: true, module: 'preserve' })
})

/** Type check `source` (a backend's `server/main.ts`) against `declarations`: the compiler's messages, in order. */
function check(declarations: string, source: string): string[] {
  const options: ts.CompilerOptions = { target: ts.ScriptTarget.ESNext, lib: ['lib.esnext.d.ts'], strict: true, noEmit: true, types: [], skipLibCheck: true }
  const host = ts.createCompilerHost(options)
  const virtual: Record<string, string> = { '/server/main.ts': source, '/types/vos.backend.d.ts': declarations }
  const exists = host.fileExists.bind(host)
  const sourceFile = host.getSourceFile.bind(host)
  host.fileExists = (file) => file in virtual || exists(file)
  host.getSourceFile = (file, languageVersion, ...rest) => (file in virtual ? ts.createSourceFile(file, virtual[file] as string, languageVersion) : sourceFile(file, languageVersion, ...rest))
  const program = ts.createProgram(Object.keys(virtual), options, host)
  return ts.getPreEmitDiagnostics(program).map((d) => ts.flattenDiagnosticMessageText(d.messageText, '\n').split('\n')[0] as string)
}

test("the TypeScript compiler takes what the contract says and refuses what it does not", () => {
  const d = emitBackendTypes(HOST_TS, 'notes', notes, [dependency])
  const good = `
    RPC.on_request('add', (params, ctx) => ({ id: params.title.length + (ctx.actor === '' ? 1 : 0) }))
    RPC.on_request('_peek', () => 3)
    const [item, err] = RPC.send_request('notes', 'add', { title: 'milk' })
    const id: number | undefined = item?.id
    RPC.send_request('app-one', 'add', { title: 'milk', tag: null })
    RPC.send_notification('added', { item: { id: 1 } })
    RPC.send_notification('_saved')
    RPC.on_notification('app-one', 'added', (params) => { const n: number = params.item.id })
    export { err, id }
  `
  expect(check(d, good)).toEqual([])
  const bad = (line: string): string[] => check(d, `${line}\nexport {}`)
  // a wrong app, method or params is an error
  expect(bad("RPC.send_request('nobody', 'add', { title: 'x' })")).not.toEqual([])
  expect(bad("RPC.send_request('notes', 'ad', { title: 'x' })")).not.toEqual([])
  expect(bad("RPC.send_request('notes', 'add', { title: 3 })")).not.toEqual([])
  expect(bad("RPC.send_request('notes', 'add', {})")).not.toEqual([])
  expect(bad("RPC.send_request('notes', 'add')")).not.toEqual([])
  // another app's private method is not offered; its public one is
  expect(bad("RPC.send_request('app-one', '_peek')")).not.toEqual([])
  // a handler: the method, its params and its result come from the contract
  expect(bad("RPC.on_request('nothing', () => 1)")).not.toEqual([])
  expect(bad("RPC.on_request('add', (params) => ({ id: params.nope }))")).not.toEqual([])
  expect(bad("RPC.on_request('add', () => ({ id: 'one' }))")).not.toEqual([])
  // a notification this app does not send, or sends with other params, and one it does not hear
  expect(bad("RPC.send_notification('nothing')")).not.toEqual([])
  expect(bad("RPC.send_notification('added', { item: { id: 'one' } })")).not.toEqual([])
  expect(bad("RPC.on_notification('app-one', '_saved', () => {})")).not.toEqual([])
})

test('writeKitTypes writes the kit types of a package that is not an app, for the frameworks it depends on', () => {
  const dir = mkdtempSync(join(tmpdir(), 'vos-kit-types-'))
  try {
    writeFileSync(join(dir, 'package.json'), JSON.stringify({ name: 'components', devDependencies: { svelte: '^5.0.0' } }))
    writeKitTypes(dir)
    expect(readFileSync(join(dir, 'types/kit-svelte.d.ts'), 'utf8')).toContain("declare module 'svelte/elements'")
    expect(existsSync(join(dir, 'types/kit-react.d.ts'))).toBe(false)
    expect(existsSync(join(dir, 'types/vos.d.ts'))).toBe(false)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
