/** Reading what an item carries: its `data` is JSON text, as the backend recorded what the agent sent. */
import type { Conversation, Item } from '../types'

/** The JSON in an item's data, or undefined when there is none (or it is not JSON). */
export function dataOf(item: Pick<Item, 'data'>): unknown {
  if (item.data === '') return undefined
  try {
    return JSON.parse(item.data) as unknown
  } catch {
    return undefined
  }
}

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const text = (value: unknown): string | undefined => (typeof value === 'string' ? value : undefined)
const records = (value: unknown): Record<string, unknown>[] => (Array.isArray(value) ? value.filter(isRecord) : [])

/** JSON text laid out to be read; text that is not JSON as it is. */
export function pretty(json: string): string {
  try {
    return JSON.stringify(JSON.parse(json), null, 2)
  } catch {
    return json
  }
}

/** A piece of what the agent said or a tool call produced, as the backend keeps it. */
export type Piece =
  | { type: 'text'; text: string }
  | { type: 'image'; mime_type: string; data: string }
  | { type: 'link'; uri: string; name: string }
  | { type: 'resource'; uri: string; text: string }
  /** A change to a file: `patch` is a unified diff. */
  | { type: 'diff'; path: string; patch: string; new_file: boolean }
  | { type: 'terminal'; terminal_id: string }

/** A piece as read from JSON: one of the kinds above with what it needs, or undefined. */
export function pieceOf(value: unknown): Piece | undefined {
  if (!isRecord(value)) return undefined
  const { type } = value
  if (type === 'text') return { type, text: text(value.text) ?? '' }
  if (type === 'image' && text(value.data) !== undefined) return { type, mime_type: text(value.mime_type) ?? 'image/png', data: text(value.data) ?? '' }
  if (type === 'link' && text(value.uri) !== undefined) return { type, uri: text(value.uri) ?? '', name: text(value.name) ?? text(value.uri) ?? '' }
  if (type === 'resource') return { type, uri: text(value.uri) ?? '', text: text(value.text) ?? '' }
  if (type === 'diff') return { type, path: text(value.path) ?? '', patch: text(value.patch) ?? '', new_file: value.new_file === true }
  if (type === 'terminal') return { type, terminal_id: text(value.terminal_id) ?? '' }
  return undefined
}

/** Whether a link may be opened from the page: a web address. The agent's own (a file on its machine) is shown, not opened. */
export const opens = (uri: string): boolean => /^https?:\/\//i.test(uri)

export type ToolStatus = 'pending' | 'in_progress' | 'completed' | 'failed'

/** A tool call of the agent's: what kind of thing it does, where it is, and what it produced. */
export interface ToolCall {
  kind?: string
  status?: ToolStatus
  content: Piece[]
  locations: { path: string; line?: number }[]
  /** Its input and output as the agent has them, JSON text. */
  input?: string
  output?: string
}

const STATUSES: readonly string[] = ['pending', 'in_progress', 'completed', 'failed']

export function toolOf(item: Pick<Item, 'data'>): ToolCall {
  const data = dataOf(item)
  if (!isRecord(data)) return { content: [], locations: [] }
  const status = text(data.status)
  return {
    kind: text(data.kind),
    status: status !== undefined && STATUSES.includes(status) ? (status as ToolStatus) : undefined,
    content: (Array.isArray(data.content) ? data.content : []).map(pieceOf).filter((p): p is Piece => p !== undefined),
    locations: records(data.locations).map((l) => ({ path: text(l.path) ?? '', ...(typeof l.line === 'number' ? { line: l.line } : {}) })),
    input: text(data.input),
    output: text(data.output)
  }
}

/** A step of the agent's plan. */
export interface PlanEntry {
  content: string
  priority: string
  status: string
}

export function planOf(item: Pick<Item, 'data'>): PlanEntry[] {
  const data = dataOf(item)
  return records(isRecord(data) ? data.entries : undefined).map((e) => ({
    content: text(e.content) ?? '',
    priority: text(e.priority) ?? 'medium',
    status: text(e.status) ?? 'pending'
  }))
}

/** An answer the agent offers to a permission it asks for. */
export interface PermissionOption {
  option_id: string
  name: string
  /** `allow_once`, `allow_always`, `reject_once` or `reject_always`. */
  kind: string
}

/** A permission the agent asked for: what kind of thing it wants to do, the answers it offers, and the one picked. */
export interface Permission {
  kind?: string
  options: PermissionOption[]
  chosen?: PermissionOption
}

export function permissionOf(item: Pick<Item, 'data'>): Permission {
  const data = dataOf(item)
  if (!isRecord(data)) return { options: [] }
  const options = records(data.options).map((o) => ({ option_id: text(o.option_id) ?? '', name: text(o.name) ?? '', kind: text(o.kind) ?? '' }))
  const chosen = options.find((o) => o.option_id === data.chosen)
  return { kind: text(data.kind), options, ...(chosen ? { chosen } : {}) }
}

/** The kit's icon for what a tool call does (ACP's tool kinds); a tool of no kind it names has the generic one. */
export function iconOf(kind: string | undefined): string {
  switch (kind) {
    case 'execute':
      return 'terminal'
    case 'search':
    case 'fetch':
      return 'magnifyingglass'
    case 'edit':
      return 'square.and.pencil'
    case 'read':
      return 'doc'
    case 'delete':
      return 'trash'
    case 'move':
      return 'folder'
    case 'think':
      return 'sparkles'
    default:
      return 'gear'
  }
}

/**
 * What the line over the composer says while the agent's turn runs: that it is at work, and the tool it is running
 * (or how many). Nothing when no turn runs, or while the turn waits for the person to answer a permission (the
 * question is in the chat, and the composer says so).
 */
export function statusOf(conversation: Pick<Conversation, 'busy' | 'waiting' | 'items'>): string | null {
  if (!conversation.busy || conversation.waiting) return null
  // a turn that ended left nothing undone: a tool call not done is one of this turn's, still running
  const running = conversation.items.filter((item) => item.kind === 'tool' && !item.done)
  if (running.length === 0) return 'Thinking…'
  if (running.length === 1) return `Thinking… · ${running[0]!.name || 'A tool'}`
  return `Thinking… · ${running.length} tools running`
}

/** A step of the agent's turn that is not its words: a tool call, or what it thought. */
export const isStep = (item: Pick<Item, 'kind'>): boolean => item.kind === 'tool' || item.kind === 'reasoning'

/** What the chat shows, in order: an item of its own, or a run of steps one after another as one row. */
export type Entry = { item: Item; steps?: undefined } | { item?: undefined; steps: Item[] }

/** The items as the chat shows them: each run of steps (two or more) is one row; what the agent says, a permission,
 *  a plan, a note or the person's words ends a run. */
export function entriesOf(items: readonly Item[]): Entry[] {
  const entries: Entry[] = []
  let run: Item[] = []
  const end = (): void => {
    if (run.length === 1) entries.push({ item: run[0]! })
    else if (run.length > 1) entries.push({ steps: run })
    run = []
  }
  for (const item of items) {
    if (isStep(item)) run.push(item)
    else {
      end()
      entries.push({ item })
    }
  }
  end()
  return entries
}

/** What a step did, as its row says it: the verb, and what it counts. */
type Did = 'created' | 'edit' | 'delete' | 'read' | 'execute' | 'search' | 'fetch' | 'move' | 'think' | 'other'
const DONE: Record<Did, (n: number) => string> = {
  created: (n) => `Created ${n} ${n === 1 ? 'file' : 'files'}`,
  edit: (n) => `Edited ${n} ${n === 1 ? 'file' : 'files'}`,
  delete: (n) => `Deleted ${n} ${n === 1 ? 'file' : 'files'}`,
  read: (n) => `Read ${n} ${n === 1 ? 'file' : 'files'}`,
  execute: (n) => `Ran ${n} ${n === 1 ? 'command' : 'commands'}`,
  search: (n) => `Searched ${n === 1 ? 'once' : `${n} times`}`,
  fetch: (n) => `Fetched ${n} ${n === 1 ? 'page' : 'pages'}`,
  move: (n) => `Moved ${n} ${n === 1 ? 'file' : 'files'}`,
  think: (n) => `Thought ${n === 1 ? 'once' : `${n} times`}`,
  other: (n) => `Used ${n} ${n === 1 ? 'tool' : 'tools'}`
}

function didOf(item: Item): Did {
  if (item.kind === 'reasoning') return 'think'
  const tool = toolOf(item)
  // an edit that made a file the agent had not got is a file created
  if (tool.kind === 'edit' && tool.content.some((p) => p.type === 'diff' && p.new_file)) return 'created'
  return tool.kind !== undefined && tool.kind in DONE && tool.kind !== 'created' ? (tool.kind as Did) : 'other'
}

/** The title of a step: a tool call's own, or `Thought`. */
export const titleOf = (item: Item): string => (item.kind === 'reasoning' ? 'Thought' : item.name || 'A tool')

/** The icon of a step: its tool kind's, or the one for thinking. */
export const stepIconOf = (item: Item): string => (item.kind === 'reasoning' ? iconOf('think') : iconOf(toolOf(item).kind))

/**
 * A run of steps as one row says it: one step by its title; more counted by what they did, in the order each first
 * came (`Ran 3 commands · Read 2 files`). Its icon is the kind most of them are (the latest of those that tie), and
 * `failed` counts the tool calls that failed.
 */
export function summaryOf(steps: readonly Item[]): { text: string; icon: string; failed: number } {
  const failed = steps.filter((s) => s.kind === 'tool' && toolOf(s).status === 'failed').length
  if (steps.length === 1) return { text: titleOf(steps[0]!), icon: stepIconOf(steps[0]!), failed }
  const counts = new Map<Did, number>()
  const latest = new Map<Did, number>()
  steps.forEach((step, i) => {
    const did = didOf(step)
    counts.set(did, (counts.get(did) ?? 0) + 1)
    latest.set(did, i)
  })
  const most = [...counts.keys()].reduce((a, b) => {
    const [ca, cb] = [counts.get(a)!, counts.get(b)!]
    return cb > ca || (cb === ca && latest.get(b)! > latest.get(a)!) ? b : a
  })
  const icon = most === 'created' ? iconOf('edit') : iconOf(most)
  return { text: [...counts].map(([did, n]) => DONE[did](n)).join(' · '), icon, failed }
}
