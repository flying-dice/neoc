/** Reading what an item carries: its `data` is JSON text, as the backend recorded what the agent sent. */
type Item = VosApp.Agents.Item

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
