/**
 * Keyed tree operations. Nodes are addressed by path: the keys from the root, "/"-joined, with an
 * `@slot` segment when the node sits in a named slot (`main/list/@row/item/delete`). Paths are stable across
 * renders because keys are. Works on plain objects and on Svelte `$state` proxies alike, so applying an op to
 * the reactive tree gives fine-grained updates: a node keeps its component instance because its path is its
 * identity.
 */
import type { Handler, Node, Op } from '../protocol'

/** Raised when an op cannot be applied; the view answers it with `view.sync` (the protocol never guesses). */
export class OpError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'OpError'
  }
}

export const MAX_DEPTH = 64
export const MAX_NODES = 10_000

export const childPath = (parentPath: string, key: string, slot?: string): string =>
  slot ? `${parentPath}/@${slot}/${key}` : `${parentPath}/${key}`

const listOf = (node: Node, slot?: string): Node[] =>
  slot ? ((node.slots ??= {})[slot] ??= []) : (node.children ??= [])

/** Resolve a path to its node and the sibling list that holds it (null for the root). */
export function locate(root: Node, path: string): { node: Node; list: Node[] | null } | null {
  const segs = path.split('/')
  if (segs[0] !== root.key) return null
  let node = root
  let list: Node[] | null = null
  for (let i = 1; i < segs.length; i++) {
    let slot: string | undefined
    if (segs[i].startsWith('@')) {
      slot = segs[i].slice(1)
      i++
    }
    const siblings: Node[] | undefined = slot ? node.slots?.[slot] : node.children
    const next = siblings?.find((c) => c.key === segs[i])
    if (!next || !siblings) return null
    node = next
    list = siblings
  }
  return { node, list }
}

/** First node (depth-first) with this key, and its path. Used by `focus` / `scrollTo` addressed by key. */
export function findByKey(root: Node, key: string): { node: Node; path: string } | null {
  const visit = (node: Node, path: string): { node: Node; path: string } | null => {
    if (node.key === key) return { node, path }
    for (const c of node.children ?? []) {
      const hit = visit(c, childPath(path, c.key))
      if (hit) return hit
    }
    for (const [slot, list] of Object.entries(node.slots ?? {}))
      for (const c of list) {
        const hit = visit(c, childPath(path, c.key, slot))
        if (hit) return hit
      }
    return null
  }
  return visit(root, root.key)
}

/** Count nodes and measure depth: the limits the host enforces on every tree it emits. */
export function measure(root: Node): { nodes: number; depth: number } {
  let nodes = 0
  let depth = 0
  const visit = (n: Node, d: number): void => {
    nodes++
    depth = Math.max(depth, d)
    for (const c of n.children ?? []) visit(c, d + 1)
    for (const list of Object.values(n.slots ?? {})) for (const c of list) visit(c, d + 1)
  }
  visit(root, 1)
  return { nodes, depth }
}

/**
 * Apply one op in place. `set`, `on`, `remove` of a missing node and `replace` are idempotent; a broken
 * address throws `OpError`.
 */
export function applyOp(root: Node, op: Op): void {
  switch (op.op) {
    case 'set': {
      const hit = locate(root, op.path)
      if (!hit) throw new OpError(`no node at ${op.path}`)
      const props = (hit.node.props ??= {})
      for (const [k, v] of Object.entries(op.props)) {
        if (v === null) delete props[k]
        else props[k] = v
      }
      return
    }
    case 'on': {
      const hit = locate(root, op.path)
      if (!hit) throw new OpError(`no node at ${op.path}`)
      const on = (hit.node.on ??= {})
      for (const [k, v] of Object.entries(op.on)) {
        if (v === null) delete on[k]
        else on[k] = v as Handler
      }
      return
    }
    case 'insert': {
      const parent = locate(root, op.parent)
      if (!parent) throw new OpError(`no parent at ${op.parent}`)
      const list = listOf(parent.node, op.slot)
      if (list.some((n) => n.key === op.node.key))
        throw new OpError(`duplicate key "${op.node.key}"`)
      const at = op.before === null ? list.length : list.findIndex((n) => n.key === op.before)
      if (at < 0) throw new OpError(`no sibling "${op.before}"`)
      list.splice(at, 0, op.node)
      return
    }
    case 'remove': {
      const hit = locate(root, op.path)
      if (!hit?.list) return // already gone: idempotent
      hit.list.splice(hit.list.indexOf(hit.node), 1)
      return
    }
    case 'move': {
      const hit = locate(root, op.path)
      if (!hit?.list) throw new OpError(`no node at ${op.path}`)
      const before = op.before === null ? null : hit.list.find((n) => n.key === op.before)
      if (before === undefined) throw new OpError(`no sibling "${op.before}"`)
      const [node] = hit.list.splice(hit.list.indexOf(hit.node), 1)
      hit.list.splice(before === null ? hit.list.length : hit.list.indexOf(before), 0, node)
      return
    }
    case 'replace': {
      const hit = locate(root, op.path)
      if (!hit?.list) throw new OpError(`cannot replace the root or a missing node: ${op.path}`)
      hit.list[hit.list.indexOf(hit.node)] = op.node
      return
    }
  }
}

export function applyOps(root: Node, ops: Op[]): void {
  for (const op of ops) applyOp(root, op)
}
