/** Reading what an item carries: its `data` is JSON text, as the agent sent it or as the backend recorded it. */
import type { Item } from 'app:rpc'

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

/** JSON text laid out to be read; text that is not JSON as it is. */
export function pretty(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2)
  } catch {
    return text
  }
}

/** A tool call: whether one of this machine's apps ran it, and what came of it. */
export interface ToolOutcome {
  /** A tool of an app on this machine, run here; otherwise the agent's own, run where the agent is. */
  ours: boolean
  result?: string
  failed: boolean
}

export function toolOutcome(item: Pick<Item, 'data'>): ToolOutcome {
  const data = dataOf(item)
  if (!isRecord(data)) return { ours: false, failed: false }
  return {
    ours: data.ours === true,
    result: typeof data.result === 'string' ? data.result : undefined,
    failed: typeof data.error === 'string'
  }
}

/** A question the agent stopped at: whether it still waits for its answer. */
export function questionStatus(item: Pick<Item, 'data'>): 'open' | 'resolved' | 'cancelled' {
  const data = dataOf(item)
  const status = isRecord(data) ? data.status : undefined
  return status === 'resolved' || status === 'cancelled' ? status : 'open'
}

/** What went wrong in a subagent, when something did. */
export function failureOf(item: Pick<Item, 'data'>): string | undefined {
  const data = dataOf(item)
  return isRecord(data) && typeof data.error === 'string' ? data.error : undefined
}
