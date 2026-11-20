/**
 * RFC 6902 JSON Patch, the subset a `STATE_DELTA` uses: `add`, `replace`, `remove` (plus `test`, which checks).
 * Pure: the document is cloned, never mutated.
 */
import type { JsonPatchOperation } from '@ag-ui/core'

export class PatchError extends Error {}

const unescape = (token: string): string => token.replace(/~1/g, '/').replace(/~0/g, '~')

/** Members a patch from an agent may never address: writing through them changes every object. */
const FORBIDDEN = new Set(['__proto__', 'constructor', 'prototype'])

function tokens(pointer: string): string[] {
  if (pointer === '') return []
  if (!pointer.startsWith('/')) throw new PatchError(`bad JSON pointer "${pointer}"`)
  const out = pointer.slice(1).split('/').map(unescape)
  for (const token of out)
    if (FORBIDDEN.has(token)) throw new PatchError(`refused member "${token}" in ${pointer}`)
  return out
}

function index(token: string, length: number, allowEnd: boolean): number {
  if (allowEnd && token === '-') return length
  if (!/^(0|[1-9]\d*)$/.test(token)) throw new PatchError(`bad array index "${token}"`)
  const i = Number(token)
  if (i > length || (!allowEnd && i === length))
    throw new PatchError(`array index ${i} out of range`)
  return i
}

type Container = Record<string, unknown> | unknown[]

function parentOf(doc: unknown, path: string[]): { parent: Container; key: string } {
  let node = doc
  for (const token of path.slice(0, -1)) {
    if (Array.isArray(node)) node = node[index(token, node.length, false)]
    else if (node && typeof node === 'object') node = (node as Record<string, unknown>)[token]
    else throw new PatchError(`no such path /${path.join('/')}`)
  }
  if (!node || typeof node !== 'object') throw new PatchError(`no such path /${path.join('/')}`)
  return { parent: node as Container, key: path[path.length - 1] }
}

/** Apply operations in order, returning a new document. Throws `PatchError` when one cannot apply. */
export function applyPatch<T>(doc: T, ops: readonly JsonPatchOperation[]): T {
  let result: unknown = structuredClone(doc)
  for (const op of ops) {
    const path = tokens(op.path)
    if (op.op === 'test') {
      const { parent, key } = parentOf(result, path)
      const actual = Array.isArray(parent) ? parent[index(key, parent.length, false)] : parent[key]
      if (JSON.stringify(actual) !== JSON.stringify(op.value))
        throw new PatchError(`test failed at ${op.path}`)
      continue
    }
    if (op.op !== 'add' && op.op !== 'replace' && op.op !== 'remove')
      throw new PatchError(`unsupported operation "${op.op}"`)
    if (path.length === 0) {
      if (op.op === 'remove') throw new PatchError('cannot remove the document root')
      result = structuredClone(op.value)
      continue
    }
    const { parent, key } = parentOf(result, path)
    if (Array.isArray(parent)) {
      if (op.op === 'add')
        parent.splice(index(key, parent.length, true), 0, structuredClone(op.value))
      else if (op.op === 'replace')
        parent[index(key, parent.length, false)] = structuredClone(op.value)
      else parent.splice(index(key, parent.length, false), 1)
    } else if (op.op === 'add') parent[key] = structuredClone(op.value)
    else if (!(key in parent)) throw new PatchError(`no such member ${op.path}`)
    else if (op.op === 'replace') parent[key] = structuredClone(op.value)
    else delete parent[key]
  }
  return result as T
}
