/**
 * The component registry: the renderer's one extension point. A node's `type` maps to an adapter component that
 * knows both the wire props and the kit component it wraps; an unknown type renders `Unsupported` (or the node's
 * own `fallback` slot), so a client older than the app degrades instead of showing holes.
 *
 * Wave B1 adds the full tier-1 kit by calling `register` once per component (see `kit.ts`); nothing else changes.
 *
 *   const registry = createRegistry()
 *   registry.register('Gauge', { component: Gauge, since: '1.2' })
 *   registry.register('Spacer', Spacer)               // shorthand: just the component
 *
 * An adapter receives `{ node, path, scope }`, reads its props through `useView().view.evaluate(...)` inside
 * `$derived`, renders children through the `Children` atom, and passes the derived test id to the kit component.
 */
import type { Component, Snippet } from 'svelte'
import type { Node } from './protocol'
import type { Scope } from './core/view'

export interface AdapterProps {
  node: Node
  /** The node's key path from the window root (`main/list/@row/item`). */
  path: string
  scope: Scope
}

/** What a decorator wraps an adapter with: the node, and the adapter itself as `children`. */
export interface DecoratorProps {
  node: Node
  path: string
  scope: Scope
  children: Snippet
}

/**
 * True when a node needs the decorator: it has a `contextMenu` slot or the common `drag` / `drop` props. Plain nodes
 * are rendered with no wrapper at all.
 */
export const needsDecorator = (node: Node): boolean =>
  (node.slots?.contextMenu?.length ?? 0) > 0 ||
  node.props?.drag !== undefined ||
  node.props?.drop !== undefined

export interface ComponentSpec {
  component: Component<AdapterProps>
  /** Kit version the component appeared in (informational; the host validates `kit` at install). */
  since?: string
  /** Names of the props this component treats as two-way `Binding`s. */
  bindings?: readonly string[]
}

export class ComponentRegistry {
  private readonly specs = new Map<string, ComponentSpec>()

  /**
   * The component that adds the common `contextMenu` slot and `drag` / `drop` behaviour around any adapter. Set by
   * the kit (`registerControls`); `Node` uses it for nodes that `needsDecorator`.
   */
  decorator: Component<DecoratorProps> | null = null

  /** Register (or replace) the adapter for a catalogue type. */
  register(type: string, spec: ComponentSpec | Component<AdapterProps>): this {
    this.specs.set(type, typeof spec === 'function' ? { component: spec } : spec) // a Svelte 5 component is a function
    return this
  }

  unregister(type: string): boolean {
    return this.specs.delete(type)
  }

  get(type: string): ComponentSpec | undefined {
    return this.specs.get(type)
  }

  has(type: string): boolean {
    return this.specs.has(type)
  }

  /** The registered types, sorted (the contract test compares this with the catalogue). */
  types(): string[] {
    return [...this.specs.keys()].sort()
  }
}

export const createRegistry = (): ComponentRegistry => new ComponentRegistry()
