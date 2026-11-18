import { describe, expect, test } from 'bun:test'
import type { Node, Op } from '../protocol'
import { applyOp, applyOps, findByKey, locate, measure, OpError } from './tree'

const tree = (): Node => ({
  type: 'Stack',
  key: 'main',
  children: [
    { type: 'Text', key: 'a', props: { text: 'A' } },
    { type: 'Text', key: 'b', props: { text: 'B' } },
    {
      type: 'List',
      key: 'list',
      slots: { row: [{ type: 'Row', key: 'item', props: { title: { $ref: 'row.title' } }, children: [{ type: 'Button', key: 'del', on: { tap: true } }] }] }
    }
  ]
})
const keys = (n: Node): string[] => (n.children ?? []).map((c) => c.key)

describe('addressing', () => {
  test('paths are keys from the root with @slot segments', () => {
    const t = tree()
    expect(locate(t, 'main')?.node).toBe(t)
    expect(locate(t, 'main/a')?.node.props?.text).toBe('A')
    expect(locate(t, 'main/list/@row/item/del')?.node.type).toBe('Button')
    expect(locate(t, 'main/ghost')).toBeNull()
    expect(locate(t, 'other/a')).toBeNull()
    expect(locate(t, 'main/list/item')).toBeNull() // a slot child needs its @slot segment
  })
  test('findByKey searches children and slots', () => {
    expect(findByKey(tree(), 'del')?.path).toBe('main/list/@row/item/del')
    expect(findByKey(tree(), 'nope')).toBeNull()
  })
  test('measure counts nodes and depth', () => {
    expect(measure(tree())).toEqual({ nodes: 6, depth: 4 })
  })
})

describe('ops', () => {
  test('set merges props and null removes one; set is idempotent', () => {
    const t = tree()
    const op: Op = { op: 'set', path: 'main/a', props: { text: 'A2', style: 'title1' } }
    applyOp(t, op)
    applyOp(t, op)
    expect(t.children![0].props).toEqual({ text: 'A2', style: 'title1' })
    applyOp(t, { op: 'set', path: 'main/a', props: { style: null } })
    expect(t.children![0].props).toEqual({ text: 'A2' })
  })
  test('on sets and clears handler flags', () => {
    const t = tree()
    applyOp(t, { op: 'on', path: 'main/a', on: { tap: [{ do: 'toggle', ref: 'client.x' }] } })
    expect(t.children![0].on?.tap).toEqual([{ do: 'toggle', ref: 'client.x' }])
    applyOp(t, { op: 'on', path: 'main/a', on: { tap: null } })
    expect(t.children![0].on).toEqual({})
  })
  test('insert before a key, append with null, into a slot', () => {
    const t = tree()
    applyOp(t, { op: 'insert', parent: 'main', before: 'b', node: { type: 'Text', key: 'x' } })
    applyOp(t, { op: 'insert', parent: 'main', before: null, node: { type: 'Text', key: 'z' } })
    expect(keys(t)).toEqual(['a', 'x', 'b', 'list', 'z'])
    applyOp(t, { op: 'insert', parent: 'main/list', slot: 'empty', before: null, node: { type: 'Text', key: 'none' } })
    expect(locate(t, 'main/list/@empty/none')).not.toBeNull()
  })
  test('insert with a duplicate key or a missing sibling is an OpError', () => {
    const t = tree()
    expect(() => applyOp(t, { op: 'insert', parent: 'main', before: null, node: { type: 'Text', key: 'a' } })).toThrow(OpError)
    expect(() => applyOp(t, { op: 'insert', parent: 'main', before: 'ghost', node: { type: 'Text', key: 'q' } })).toThrow(OpError)
    expect(() => applyOp(t, { op: 'insert', parent: 'main/ghost', before: null, node: { type: 'Text', key: 'q' } })).toThrow(OpError)
  })
  test('remove is idempotent', () => {
    const t = tree()
    applyOp(t, { op: 'remove', path: 'main/a' })
    applyOp(t, { op: 'remove', path: 'main/a' })
    expect(keys(t)).toEqual(['b', 'list'])
  })
  test('move reorders among siblings; a bad target leaves the tree as it was', () => {
    const t = tree()
    applyOp(t, { op: 'move', path: 'main/list', before: 'a' })
    expect(keys(t)).toEqual(['list', 'a', 'b'])
    applyOp(t, { op: 'move', path: 'main/list', before: null })
    expect(keys(t)).toEqual(['a', 'b', 'list'])
    expect(() => applyOp(t, { op: 'move', path: 'main/a', before: 'ghost' })).toThrow(OpError)
    expect(keys(t)).toEqual(['a', 'b', 'list'])
  })
  test('replace swaps the node (a type change) at the same key; the root cannot be replaced', () => {
    const t = tree()
    applyOp(t, { op: 'replace', path: 'main/b', node: { type: 'Button', key: 'b' } })
    expect(t.children![1].type).toBe('Button')
    expect(() => applyOp(t, { op: 'replace', path: 'main', node: { type: 'Stack', key: 'main' } })).toThrow(OpError)
  })
  test('a set on a missing path is an OpError (the view resyncs)', () => {
    expect(() => applyOps(tree(), [{ op: 'set', path: 'main/ghost', props: {} }])).toThrow(OpError)
  })
})
