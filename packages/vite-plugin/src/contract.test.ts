import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseContract, toolName, type Contract } from './contract.ts'

const doc = (methods: unknown[], components: Record<string, unknown> = {}, openrpc = '1.4.0'): string =>
  JSON.stringify({ openrpc, info: { title: 'T', version: '1.0.0' }, methods, components: { schemas: components } })

const request = (name: string, extra: Record<string, unknown> = {}) => ({ name, params: [], result: { name: 'r', schema: { type: 'string' } }, ...extra })
const notification = (name: string, extra: Record<string, unknown> = {}) => ({ name, params: [], ...extra })

describe('parseContract', () => {
  test('a method with a result is a request, one without is a notification', () => {
    const c = parseContract(doc([request('add'), notification('added')]))
    expect(c.requests.map((r) => r.name)).toEqual(['add'])
    expect(c.notifications.map((n) => n.name)).toEqual(['added'])
  })

  test('`_name` is private, `name` public', () => {
    const c = parseContract(doc([request('add'), request('_peek'), notification('_tick')]))
    expect(c.requests.map((r) => [r.name, r.private])).toEqual([['add', false], ['_peek', true]])
    expect(c.notifications[0]?.private).toBe(true)
  })

  test('x-readOnly marks a request, and is refused on a notification', () => {
    expect(parseContract(doc([request('peek', { 'x-readOnly': true })])).requests[0]?.readOnly).toBe(true)
    expect(() => parseContract(doc([notification('tick', { 'x-readOnly': true })]))).toThrow('is for a request')
  })

  test('only OpenRPC 1.4', () => {
    expect(() => parseContract(doc([], {}, '1.3.2'))).toThrow('must be 1.4.x')
  })

  test('params are required, as OpenRPC says', () => {
    expect(() => parseContract(doc([{ name: 'add', result: { name: 'r', schema: { type: 'string' } } }]))).toThrow('OpenRPC requires `params`')
  })

  test('a name and its private twin may not both exist', () => {
    expect(() => parseContract(doc([request('add'), request('_add')]))).toThrow('one name is public or private, not both')
  })

  test('two methods that generate one type name are refused', () => {
    expect(() => parseContract(doc([request('add_task'), request('addTask')]))).toThrow('generates the type AddTaskParams')
  })

  test('two public requests that are one agent tool are refused', () => {
    expect(() => parseContract(doc([request('add_site_x'), request('addSiteX')]))).toThrow()
  })

  test('a component may not take a generated name', () => {
    expect(() => parseContract(doc([], { Handlers: { type: 'string' } }))).toThrow('a name the generated code uses itself')
  })

  test('an unknown extension and an unknown schema keyword are errors that say where', () => {
    expect(() => parseContract(doc([request('add', { 'x-other': 1 })]))).toThrow('methods[0].x-other')
    expect(() => parseContract(doc([request('add', { result: { name: 'r', schema: { type: 'string', format: 'email' } } })]))).toThrow(
      'methods[0].result.schema.format'
    )
  })

  test('a $ref must name a component', () => {
    expect(() => parseContract(doc([request('add', { result: { name: 'r', schema: { $ref: '#/components/schemas/Nope' } } })]))).toThrow('which components.schemas does not define')
  })
})

test('toolName: a public request is the agent tool of its snake-case name', () => {
  expect(toolName('calculate')).toBe('calculate')
  expect(toolName('addSite')).toBe('add_site')
})

describe('the fixtures', () => {
  const apps = resolve(import.meta.dir, '../../../e2e/apps')
  const read = (name: string): Contract => parseContract(readFileSync(join(apps, name, 'openrpc.json'), 'utf8'), `${name}/openrpc.json`)

  test('the two task lists have one page API: the same private methods and the same schemas', () => {
    const page = (c: Contract): unknown => ({
      requests: c.requests.filter((r) => r.private),
      notifications: c.notifications,
      components: c.components
    })
    expect(page(read('tasks-react'))).toEqual(page(read('tasks-svelte')))
  })
})
