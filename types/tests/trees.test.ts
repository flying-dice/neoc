/**
 * Runtime contract: the trees the Calculator and Notes examples render (committed under the codegen fixtures,
 * produced by running the Lua against the generated `ui.lua`) only use components, props, slots and events the
 * generated `COMPONENTS` table knows, and the table agrees with the protocol constants.
 */
import { describe, expect, test } from 'bun:test'
import calculator from '../../crates/vos-codegen/tests/fixtures/trees/calculator.json'
import notes from '../../crates/vos-codegen/tests/fixtures/trees/notes.json'
import { CLIENT_METHODS, COMPONENTS, FUNCTIONS, HOST_METHODS, KIT_VERSION, PROTO_VERSION } from '../generated/agentos'
import type { ComponentName, Node } from '../generated/agentos'

type Meta = (typeof COMPONENTS)[ComponentName]

function walk(node: Node, visit: (node: Node, meta: Meta) => void): void {
  const meta = COMPONENTS[node.type as ComponentName]
  expect(meta, `unknown component ${node.type} at ${node.key}`).toBeDefined()
  visit(node, meta)
  for (const child of node.children ?? []) walk(child, visit)
  for (const children of Object.values(node.slots ?? {})) for (const child of children) walk(child, visit)
}

const isRef = (value: unknown): value is { $ref: string } =>
  typeof value === 'object' && value !== null && '$ref' in value

describe.each([
  ['calculator', calculator],
  ['notes', notes]
])('the %s tree', (_name, rendered) => {
  const tree = rendered.tree as unknown as Node

  test('uses only components the schema knows', () => {
    let count = 0
    walk(tree, () => count++)
    expect(count).toBeGreaterThan(5)
  })

  test('uses only declared events, and interactive nodes have explicit keys', () => {
    walk(tree, (node, meta) => {
      for (const event of Object.keys(node.on ?? {})) {
        expect(meta.events as readonly string[], `${node.type}.${event}`).toContain(event)
        expect(node.key, `${node.type} handles ${event}`).not.toMatch(/^_\d+$/)
      }
      if (meta.interactive) expect(node.key, node.type).not.toMatch(/^_\d+$/)
    })
  })

  test('uses only declared slots', () => {
    walk(tree, (node, meta) => {
      for (const slot of Object.keys(node.slots ?? {})) {
        const known = (meta.slots as readonly string[]).some((s) => (s.endsWith('*') ? slot.startsWith(s.slice(0, -1)) : s === slot))
        expect(known, `${node.type} slot ${slot}`).toBe(true)
      }
    })
  })

  test('binds only writable refs to Binding props', () => {
    walk(tree, (node, meta) => {
      for (const prop of Object.keys(meta.bindings)) {
        const value = node.props?.[prop]
        if (value === undefined) continue
        expect(isRef(value), `${node.type}.${prop} must be a ref`).toBe(true)
        expect((value as { $ref: string }).$ref).toMatch(/^(client|state|src|row)\./)
      }
    })
  })

  test('uses only functions from the closed set', () => {
    const calls: string[] = []
    JSON.stringify(tree, (_k, v) => {
      if (v && typeof v === 'object' && '$fn' in v) calls.push(v.$fn)
      return v
    })
    for (const call of calls) expect(Object.keys(FUNCTIONS)).toContain(call)
  })
})

test('the notes example declares persisted client state and three sources', () => {
  expect(Object.keys(notes.sources)).toHaveLength(3)
  expect(notes.client.folder.persist).toBe('app')
})

test('versions and method tables are announced', () => {
  expect(PROTO_VERSION).toBe(1)
  expect(KIT_VERSION).toBe('1.0')
  expect(CLIENT_METHODS).toContain('view.event')
  expect(HOST_METHODS).toContain('view.ops')
})

test('the component table marks the interactive controls', () => {
  for (const name of ['Button', 'Toggle', 'TextField', 'TextEditor', 'Picker', 'Slider'] as const) {
    expect(COMPONENTS[name].interactive).toBe(true)
  }
  expect(COMPONENTS.Text.interactive).toBe(false)
  expect(COMPONENTS.TextEditor.bindings.text).toBe('text')
  expect(COMPONENTS.Toggle.bindings.on).toBe('scalar')
})
