/**
 * Navigation as client-managed state: a `NavigationStack`'s path is a list of routes
 * (`{name, ...params}`), authoritative on the client, so push and pop never wait for the host. This module is the
 * framework-free half: the pure stack operations and the registry that routes the `nav.push` / `nav.pop` /
 * `nav.popToRoot` client actions to the stack they mean. The Svelte adapter registers each mounted stack here.
 *
 *  - `nav.push { route }` from a node goes to the innermost stack that contains that node (its path is a prefix
 *    of the node's path), or to the stack named by `stack` (the stack node's key).
 *  - The path is held where the stack's `path` binding points (client state, so it survives a reload when the
 *    app declares it `persist`); a stack with no binding keeps it locally.
 */
import type { Json, Route } from '../protocol'

export const MAX_DEPTH = 32

const isRecord = (v: unknown): v is Record<string, Json> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

/** A route is an object with a non-empty string `name`; params are whatever else it carries. */
export const isRoute = (v: unknown): v is Route =>
  isRecord(v) && typeof v.name === 'string' && v.name.length > 0

/** Whatever a binding holds (null, a stale value, junk from a restored session) as a clean list of routes. */
export const asPath = (value: unknown): Route[] =>
  Array.isArray(value) ? value.filter(isRoute).slice(0, MAX_DEPTH) : []

export function pushRoute(path: readonly Route[], route: Route): Route[] {
  return path.length >= MAX_DEPTH ? [...path] : [...path, route]
}
export const popRoute = (path: readonly Route[]): Route[] => path.slice(0, -1)
export const popToRoot = (): Route[] => []

/** True when `a` and `b` are the same route (name and params), so pushing the top route again can be ignored. */
export const sameRoute = (a: Route, b: Route): boolean => JSON.stringify(a) === JSON.stringify(b)

/** A mounted stack, as the action runner sees it. */
export interface NavHandle {
  /** The stack node's key (what `nav.push {stack}` names). */
  readonly key: string
  /** The stack node's key path from the window root. */
  readonly path: string
  push(route: Route): void
  pop(): void
  popToRoot(): void
}

export class NavRegistry {
  private handles: NavHandle[] = []

  /** Register a mounted stack; returns what to call when it unmounts. */
  register(handle: NavHandle): () => void {
    this.handles.push(handle)
    return () => {
      this.handles = this.handles.filter((h) => h !== handle)
    }
  }

  /**
   * The stack an action addresses. `stack` names one by key (the most recently mounted wins); otherwise the
   * innermost stack containing the node at `from`; otherwise the only stack there is.
   */
  resolve(from: string, stack?: string): NavHandle | undefined {
    if (stack !== undefined) return [...this.handles].reverse().find((h) => h.key === stack)
    const containing = this.handles
      .filter((h) => from === h.path || from.startsWith(`${h.path}/`))
      .sort((a, b) => b.path.length - a.path.length)
    if (containing.length) return containing[0]
    return this.handles.length === 1 ? this.handles[0] : undefined
  }

  get size(): number {
    return this.handles.length
  }
}

/** One screen of a mounted stack. The root has no route. A popped screen stays (`leaving`) until its slide ends. */
export interface StackEntry {
  id: string
  route: Route | null
  leaving?: boolean
}

/**
 * Bring the screens in step with the path: keep the common prefix, mark the screens above it as leaving (they
 * slide out, then `removeEntry`), and add the new ones on top. Screens already leaving stay where they are.
 * Entries keep their id for as long as they stay, so a screen keeps its scroll and state while others come and go.
 */
export function reconcile(
  entries: readonly StackEntry[],
  routes: readonly Route[],
  nextId: () => string
): StackEntry[] {
  const live = entries.filter((e) => !e.leaving)
  const root = live[0] ?? { id: nextId(), route: null }
  let common = 0
  while (
    common < routes.length &&
    common + 1 < live.length &&
    live[common + 1].route &&
    sameRoute(live[common + 1].route!, routes[common])
  )
    common++
  const kept = [root, ...live.slice(1, common + 1)]
  const dropped = live.slice(common + 1).map((e) => ({ ...e, leaving: true }))
  const added = routes.slice(common).map((route) => ({ id: nextId(), route }))
  return [...kept, ...entries.filter((e) => e.leaving), ...dropped, ...added]
}

export const removeEntry = (entries: readonly StackEntry[], id: string): StackEntry[] =>
  entries.filter((e) => e.id !== id)
