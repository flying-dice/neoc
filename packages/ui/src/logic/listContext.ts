/** What a `List` tells the `ListSection`s and rows inside it (Svelte context). */
export type ListStyle = 'plain' | 'inset' | 'insetGrouped' | 'grouped' | 'sidebar'

export interface ListStyleContext {
  readonly style: ListStyle
  /** Single or multiple selection: sections are groups of options, not lists of items. */
  readonly selectable: boolean
}

export const LIST_CONTEXT = Symbol('agentos.ui.list')
