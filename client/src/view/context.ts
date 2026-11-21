/**
 * What every adapter reads from its window: the view core, the app id (test ids), the registry, and `render`,
 * the recursion. `render` is a snippet defined by `ViewHost` (which owns `Node`), so adapters in a lower atomic
 * layer can render child nodes without importing the component that renders them.
 */
import { getContext, setContext, type Snippet } from 'svelte'
import type { Scope, ViewCore } from './core/view'
import type { Node, RowKey } from './protocol'
import type { ComponentRegistry } from './registry'

const VIEW = Symbol('vos.view')
const LIST = Symbol('vos.view.list')

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

/** What a `List` shares with its rows: selection, taps, position in the set and whether rows can be reordered. */
export interface ListContext {
  mode: () => 'none' | 'single' | 'multi'
  isSelected(tag: unknown): boolean
  select(tag: unknown): void
  /** A row was tapped: the list's own `onTap {key}`. */
  tap(key: RowKey): void
  /** The list has an `onTap` handler: its rows are tappable even without their own. */
  handlesTap(): boolean
  /** 1-based position and set size of a row (`aria-posinset` / `aria-setsize`), when the list knows them. */
  position(key: RowKey): { pos?: number; size?: number }
  /** Rows show their drag handle. */
  reorderable: () => boolean
  /** A `Form`: control rows (toggles, fields, pickers) are wrapped to look like rows. */
  cells: () => boolean
}

export const provideList = (ctx: ListContext): void => void setContext(LIST, ctx)
export const useList = (): ListContext | undefined => getContext<ListContext | undefined>(LIST)

/**
 * Where a screen sits: the landmark it is, how to go back (a pushed screen, or a collapsed split view's column) and
 * how to bring back a hidden sidebar. A navigation stack or a split view provides one per screen or column.
 */
export interface NavLevel {
  /** 0 for the first screen of a stack or column flow. */
  depth: number
  landmark: 'main' | 'navigation' | 'region'
  back?: { label: string | undefined; go(): void }
  /** A split view's sidebar can be hidden and shown: the screens beside it get a button for it. */
  sidebar?: { shown: boolean; toggle(): void }
  /** A screen reports its title so the next one can name its back button. */
  setTitle(title: string): void
}

const NAV = Symbol('vos.view.nav')
export const provideNavLevel = (level: NavLevel): void => void setContext(NAV, level)
export const useNavLevel = (): NavLevel | undefined => getContext<NavLevel | undefined>(NAV)

/** A split view column that can move the collapsed flow forward (a row was chosen in the sidebar). */
export interface ColumnContext {
  advance(): void
}
const COLUMN = Symbol('vos.view.column')
export const provideColumn = (ctx: ColumnContext): void => void setContext(COLUMN, ctx)
export const useColumn = (): ColumnContext | undefined =>
  getContext<ColumnContext | undefined>(COLUMN)
