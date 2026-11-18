/**
 * What every adapter reads from its window: the view core, the app id (test ids), the registry, and `render`,
 * the recursion. `render` is a snippet defined by `ViewHost` (which owns `Node`), so adapters in a lower atomic
 * layer can render child nodes without importing the component that renders them.
 */
import { getContext, setContext, type Snippet } from 'svelte'
import type { Scope, ViewCore } from './core/view'
import type { Node } from './protocol'
import type { ComponentRegistry } from './registry'

const VIEW = Symbol('agentos.view')
const LIST = Symbol('agentos.view.list')

export interface ViewContext {
  view: ViewCore
  /** The app id: the prefix of every derived test id. */
  app: string
  registry: ComponentRegistry
  render: Snippet<[Node, string, Scope]>
  /** Ask the session to reconnect now (the offline banner's "Retry"). */
  retry?: () => void
}

export const provideView = (ctx: ViewContext): void => void setContext(VIEW, ctx)
export const useView = (): ViewContext => getContext<ViewContext>(VIEW)

/** Selection state a `List` shares with its `Row`s. */
export interface ListContext {
  mode: () => 'none' | 'single' | 'multi'
  isSelected(tag: unknown): boolean
  select(tag: unknown): void
}

export const provideList = (ctx: ListContext): void => void setContext(LIST, ctx)
export const useList = (): ListContext | undefined => getContext<ListContext | undefined>(LIST)
