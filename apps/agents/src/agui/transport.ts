/**
 * The host page's agent chat services (`vos/agui/*`, `client/src/shell/hostServices/agui.ts`), as the chat uses them. An
 * app's page cannot reach the host's AG-UI routes itself (no cookie, no `fetch`): every request goes by the bridge to the
 * host page, which makes it with its own session, under this app's id.
 */
import type { ResumeEntry } from './schema'

/** A turn to run: a new user message, or the answers to the open permission prompts. */
export interface RunRequest {
  threadId: string
  runId: string
  agentId: string
  messages?: { id: string; role: 'user'; content: string }[]
  resume?: ResumeEntry[]
}

export type SettingsChange = { mode: string } | { config: { id: string; value: string | boolean } }

/** What one pull of a run's events answers: the events after the one named, the last one's number, and whether that is all. */
export interface EventBatch {
  events: unknown[]
  last: number
  done: boolean
}

export interface ChatTransport {
  /** Ensure the thread (and its agent session): its snapshot. */
  thread(threadId: string, agentId: string): Promise<unknown>
  /** Start a run; resolves once its events are streaming. */
  run(input: RunRequest): Promise<void>
  /** The run's events after event number `after` (waits for one, or answers an empty batch now and then). */
  events(threadId: string, runId: string, after: number): Promise<EventBatch>
  /** Stop the turn, and resolve its open permission prompts. */
  cancel(threadId: string): Promise<void>
  /** Stop receiving the run (the host treats it as a client gone). */
  detach(threadId: string): Promise<void>
  /** Change the session's mode or a setting: the thread's state after it. */
  settings(threadId: string, change: SettingsChange): Promise<unknown>
}

/** How a page calls a host service: the generated client's `host.call`. */
export type HostCall = (
  method: `vos/${string}`,
  params?: { readonly [key: string]: unknown }
) => Promise<unknown>

const isBatch = (value: unknown): value is EventBatch =>
  typeof value === 'object' &&
  value !== null &&
  Array.isArray((value as EventBatch).events) &&
  typeof (value as EventBatch).last === 'number' &&
  typeof (value as EventBatch).done === 'boolean'

/** The transport over the bridge. */
export function hostTransport(call: HostCall): ChatTransport {
  return {
    thread: (threadId, agentId) => call('vos/agui/thread', { threadId, agentId }),
    run: async (input) => {
      await call('vos/agui/run', { ...input })
    },
    events: async (threadId, runId, after) => {
      const batch = await call('vos/agui/events', { threadId, runId, after })
      if (!isBatch(batch))
        throw new Error('The host page answered a pull of the run with something else')
      return batch
    },
    cancel: async (threadId) => {
      await call('vos/agui/cancel', { threadId })
    },
    detach: async (threadId) => {
      await call('vos/agui/detach', { threadId })
    },
    settings: (threadId, change) => call('vos/agui/settings', { threadId, change })
  }
}
