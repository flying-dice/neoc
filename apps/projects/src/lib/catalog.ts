/**
 * What an issue can be, and how each of those reads: the columns of the board, the kinds of work, the priorities. Which
 * issues a column holds and the order a project's pages are listed in are decided here, from the workspace.
 */
type Color = VosApp.Projects.Color
type Issue = VosApp.Projects.Issue
type IssueType = VosApp.Projects.IssueType
type Page = VosApp.Projects.Page
type Priority = VosApp.Projects.Priority
type Status = VosApp.Projects.Status

/** The columns of the board, left to right. `color` is a palette role. */
export const STATUSES = [
  { value: 'todo', label: 'To Do', color: 'gray' },
  { value: 'doing', label: 'In Progress', color: 'blue' },
  { value: 'done', label: 'Done', color: 'green' }
] as const satisfies readonly { value: Status; label: string; color: string }[]

/** The kinds of work. `icon` is a glyph of the icon set, `color` a palette role. */
export const TYPES = [
  { value: 'task', label: 'Task', icon: 'checkmark', color: 'blue' },
  { value: 'bug', label: 'Bug', icon: 'exclamationmark.triangle', color: 'red' },
  { value: 'story', label: 'Story', icon: 'star', color: 'green' }
] as const satisfies readonly { value: IssueType; label: string; icon: string; color: string }[]

export const PRIORITIES = [
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' }
] as const satisfies readonly { value: Priority; label: string }[]

/** The parts of a project a window can show. */
export const TABS = [
  { value: 'board', label: 'Board' },
  { value: 'issues', label: 'Issues' },
  { value: 'pages', label: 'Pages' }
] as const

export type Tab = (typeof TABS)[number]['value']

/** The colours a project can have, in the order new projects take them. */
export const COLORS: readonly Color[] = ['blue', 'purple', 'orange', 'green', 'pink', 'teal', 'indigo', 'red']

export const statusOf = (status: Status): (typeof STATUSES)[number] => STATUSES.find((s) => s.value === status) ?? STATUSES[0]
export const typeOf = (type: IssueType): (typeof TYPES)[number] => TYPES.find((t) => t.value === type) ?? TYPES[0]
export const priorityOf = (priority: Priority): (typeof PRIORITIES)[number] => PRIORITIES.find((p) => p.value === priority) ?? PRIORITIES[1]

const WEIGHT: Readonly<Record<Priority, number>> = { high: 0, medium: 1, low: 2 }

/** The issues of one column, the most pressing first; among equals, in the order they were added. */
export function column(issues: readonly Issue[], status: Status): Issue[] {
  return issues.filter((i) => i.status === status).sort((a, b) => WEIGHT[a.priority] - WEIGHT[b.priority] || a.id - b.id)
}

/** A page as the list shows it: how deep it is, and the titles of the pages it is under, outermost first. */
export interface Leaf {
  readonly page: Page
  readonly depth: number
  readonly path: readonly string[]
}

/**
 * A project's pages in the order they are listed: each page followed by the pages under it. The pages come by title
 * and keep that order among siblings. A page whose parent is gone is listed at the top, so nothing is ever hidden.
 */
export function tree(pages: readonly Page[]): Leaf[] {
  const ids = new Set(pages.map((p) => p.id))
  const under = new Map<number, Page[]>()
  for (const page of pages) {
    const parent = ids.has(page.parent) && page.parent !== page.id ? page.parent : 0
    under.set(parent, [...(under.get(parent) ?? []), page])
  }
  const leaves: Leaf[] = []
  const seen = new Set<number>()
  const walk = (parent: number, path: readonly string[]): void => {
    for (const page of under.get(parent) ?? []) {
      if (seen.has(page.id)) continue
      seen.add(page.id)
      leaves.push({ page, depth: path.length, path })
      walk(page.id, [...path, page.title])
    }
  }
  walk(0, [])
  // pages that are under each other in a circle hang from no top page: list them too
  for (const page of pages) if (!seen.has(page.id)) leaves.push({ page, depth: 0, path: [] })
  return leaves
}

/**
 * The key a project called `name` is offered: the first letters of its words, or the start of its one word, in
 * capitals (`Web Site` gives WS, `Website` gives WEB). Empty when the name has no letter to start one with.
 */
export function suggestKey(name: string): string {
  const words = name.toUpperCase().match(/[A-Z0-9]+/g) ?? []
  const text = words.length > 1 ? words.map((w) => w[0]).join('') : (words[0] ?? '').slice(0, 3)
  return text.replace(/^[0-9]+/, '').slice(0, 5)
}
