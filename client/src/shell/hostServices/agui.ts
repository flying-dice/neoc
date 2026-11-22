/**
 * Agent chat for an app's page: the `vos/agui/*` host services. An app's page runs in a sandboxed frame with no cookie and
 * no `fetch`, so it cannot reach the host's AG-UI routes (`/agui`, `crates/vos-agui`) itself; the host page makes those
 * requests for it, with its own session, and hands the results over the bridge. Every app's page is offered these
 * services (there are no permissions); what holds is that a page reaches only its own app's chats.
 *
 *   vos/agui/thread   {threadId, agentId}                       -> the thread's snapshot (ensures the thread and its agent session)
 *   vos/agui/run      {threadId, runId, agentId, messages|resume} -> {} once the host's event stream is open
 *   vos/agui/events   {threadId, runId, after}                  -> {events, last, done}: the run's events after `after`
 *   vos/agui/cancel   {threadId}                                -> {} (stops the turn and resolves its open permission prompts)
 *   vos/agui/detach   {threadId}                                -> {} (stops receiving; the host treats it as a client gone)
 *   vos/agui/settings {threadId, change}                        -> the thread's state after the change
 *
 * **Whose thread.** A page names a chat by an id of its own (`[A-Za-z0-9_-]{1,64}`); the host page sends the host
 * `<app>:<id>`, where `<app>` is the frame's app, fixed when the frame was made and never read from a message (the rule the
 * view-tree renderer had, `scopedThread`). A page therefore cannot name another app's chat, whatever id it invents. The prefix
 * is taken off again in what comes back.
 *
 * **Streaming by pull.** `postMessage` has no back-pressure, so the host page does not push a run's events into the frame:
 * the page pulls them with `vos/agui/events`, the way a WHATWG stream's reader pulls (and as MCP's tasks are started and then
 * polled over JSON-RPC). The events are AG-UI's own, unchanged but for the thread id, each numbered from 1. A pull names the
 * last number it has (`after`); what it answers is everything after that (up to {@link MAX_ANSWER_BYTES}), so a pull whose
 * answer was lost is simply asked again from the same place: nothing is skipped or repeated. Events are kept until a later
 * pull acknowledges them, at most {@link MAX_BUFFERED_BYTES} of them; at that bound the host page stops reading the response,
 * and the connection's own flow control holds the host back. A pull with nothing to give waits for the next event, the end of
 * the stream, or {@link POLL_IDLE_MS} (then it answers an empty batch, `done: false`), so no answer is ever older than that,
 * whatever time limit a call may meet on the way.
 *
 * **Teardown.** Everything a page started ends with its document (`ServiceContext.signal`: the window closes, which
 * uninstalling or disabling the app does too, or the page reloads; a reload of the app's backend does not touch it): the request is aborted, and the host does what it did when the old renderer's window closed
 * mid-answer: a run that is not waiting on a permission prompt is cancelled, one that is waiting is kept for the answer
 * (`ChatService::client_gone`). Reopening the chat asks for its snapshot again, which holds the history and any open prompt.
 */
import { z } from 'zod'
import { HostError } from '../channel'
import type { ServiceContext, ServiceRegistry } from '../hostServices'
import { SseParser } from './sse'

/** The most of a run's events kept for a page that has not acknowledged them yet, in bytes of JSON. */
export const MAX_BUFFERED_BYTES = 1024 * 1024
/** The most one `vos/agui/events` answer carries, in bytes of JSON (it always carries at least one event if there is one). */
export const MAX_ANSWER_BYTES = 256 * 1024
/** How long a pull with nothing to give waits before it answers an empty batch. */
export const POLL_IDLE_MS = 25_000
/** Runs one frame may have streaming at once (one per chat). */
export const MAX_RUNS_PER_FRAME = 4
/** The most a run's request may weigh as JSON (the bridge's limit for a backend call). */
export const MAX_RUN_BYTES = 1_000_000

const INVALID_PARAMS = -32602
const FAILED = -32000
const BUSY = -32000 // the host's own busy code (`codes::BUSY`), as the bridge uses

/** The part of `fetch` the services use (a test passes a fake host). */
export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>

export interface AguiOptions {
  fetch?: FetchLike
  /** How long an idle pull waits ({@link POLL_IDLE_MS}); a test makes it short. */
  pollIdleMs?: number
}

const pageId = z
  .string()
  .regex(/^[A-Za-z0-9_-]{1,64}$/, 'a chat id is 1 to 64 letters, digits, dashes or underscores')
const agentId = z.string().min(1).max(128)

const threadParams = z.object({ threadId: pageId, agentId }).strict()
const oneThread = z.object({ threadId: pageId }).strict()
const runParams = z
  .object({
    threadId: pageId,
    runId: pageId,
    agentId,
    messages: z.array(z.record(z.string(), z.unknown())).max(100).optional(),
    resume: z.array(z.record(z.string(), z.unknown())).max(100).optional()
  })
  .strict()
const eventsParams = z
  .object({ threadId: pageId, runId: pageId, after: z.number().int().nonnegative() })
  .strict()
const settingsParams = z
  .object({
    threadId: pageId,
    change: z.union([
      z.object({ mode: z.string().min(1).max(128) }).strict(),
      z
        .object({
          config: z
            .object({
              id: z.string().min(1).max(128),
              value: z.union([z.string().max(4096), z.boolean()])
            })
            .strict()
        })
        .strict()
    ])
  })
  .strict()

function parse<T>(schema: z.ZodType<T>, method: string, params: unknown): T {
  const parsed = schema.safeParse(params)
  if (parsed.success) return parsed.data
  const issue = parsed.error.issues[0]
  const where = issue?.path.length ? `params.${issue.path.join('.')}` : 'params'
  throw new HostError(INVALID_PARAMS, `${method}: ${where}: ${issue?.message ?? 'invalid'}`)
}

interface Sequenced {
  seq: number
  event: unknown
  bytes: number
}

/** One run's stream, as the host page holds it for the page. */
interface Stream {
  runId: string
  /** Aborts the request: the page detached, cancelled, or its document went. */
  stop: AbortController
  /** Events not yet acknowledged, in order. */
  kept: Sequenced[]
  keptBytes: number
  lastSeq: number
  /** The response has ended (or failed, which adds a `RUN_ERROR` of its own). */
  ended: boolean
  /** The pull that is waiting for something to give. */
  waiter: (() => void) | null
  /** The reader paused at the bound; called to let it read on. */
  resume: (() => void) | null
}

const encoder = new TextEncoder()
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

/** The `{ "error": "..." }` of a failed request, else its text, else its status line. */
async function httpError(res: Response): Promise<string> {
  let text = ''
  try {
    text = (await res.text()).trim()
    const body = JSON.parse(text) as unknown
    if (
      typeof body === 'object' &&
      body !== null &&
      typeof (body as { error?: unknown }).error === 'string'
    )
      return (body as { error: string }).error
  } catch {
    // not JSON: the plain text (or the status line) will do
  }
  return text.slice(0, 1000) || `${res.status} ${res.statusText || 'request failed'}`.trim()
}

/** Offer the `vos/agui/*` services on `registry` (one registry per frame: the streams it holds are that frame's). */
export function registerAgui(
  registry: ServiceRegistry,
  options: AguiOptions = {}
): ServiceRegistry {
  const fetchImpl: FetchLike = options.fetch ?? ((input, init) => globalThis.fetch(input, init))
  const idleMs = options.pollIdleMs ?? POLL_IDLE_MS
  /** The frame's streams by the host's thread id. */
  const streams = new Map<string, Stream>()

  const scoped = (context: ServiceContext, id: string): string => `${context.appId}:${id}`
  const prefix = (context: ServiceContext): string => `${context.appId}:`
  /** The page's own id for a host thread id of its app (what comes back carries the host's). */
  const unscope = (context: ServiceContext, value: unknown): unknown =>
    typeof value === 'string' && value.startsWith(prefix(context))
      ? value.slice(prefix(context).length)
      : value

  const request = (
    context: ServiceContext,
    path: string,
    body?: object,
    signal?: AbortSignal,
    stream = false
  ): Promise<Response> =>
    fetchImpl(path, {
      method: 'POST',
      credentials: 'same-origin',
      headers: {
        ...(body ? { 'content-type': 'application/json' } : {}),
        ...(stream ? { accept: 'text/event-stream' } : {})
      },
      body: body ? JSON.stringify(body) : undefined,
      signal: signal ?? context.signal
    }).catch((error: unknown) => {
      throw new HostError(FAILED, `Could not reach vos: ${messageOf(error)}`)
    })

  const json = async (res: Response): Promise<unknown> => {
    if (!res.ok) throw new HostError(FAILED, await httpError(res), { status: res.status })
    return res.json()
  }

  const wake = (s: Stream): void => {
    const waiter = s.waiter
    s.waiter = null
    waiter?.()
  }

  const keep = (context: ServiceContext, s: Stream, event: unknown): void => {
    if (typeof event === 'object' && event !== null && 'threadId' in event)
      (event as { threadId: unknown }).threadId = unscope(
        context,
        (event as { threadId: unknown }).threadId
      )
    const bytes = encoder.encode(JSON.stringify(event)).length
    s.kept.push({ seq: ++s.lastSeq, event, bytes })
    s.keptBytes += bytes
    wake(s)
  }

  const end = (s: Stream): void => {
    s.ended = true
    wake(s)
  }

  /** Read the response into the stream until it ends, pausing while the page has the bound unacknowledged. */
  async function pump(
    context: ServiceContext,
    s: Stream,
    body: ReadableStream<Uint8Array>
  ): Promise<void> {
    const reader = body.getReader()
    const decoder = new TextDecoder()
    const parser = new SseParser()
    const take = (payloads: string[]): void => {
      for (const data of payloads) {
        let event: unknown
        try {
          event = JSON.parse(data)
        } catch {
          event = { type: 'RUN_ERROR', message: 'vos sent an event that is not JSON' }
        }
        keep(context, s, event)
      }
    }
    try {
      for (;;) {
        while (s.keptBytes >= MAX_BUFFERED_BYTES && !s.stop.signal.aborted)
          await new Promise<void>((resolve) => (s.resume = resolve))
        if (s.stop.signal.aborted) return
        const { done, value } = await reader.read()
        if (done) break
        take(parser.push(decoder.decode(value, { stream: true })))
      }
      take(parser.push(decoder.decode()))
      take(parser.end())
    } catch (error) {
      if (!s.stop.signal.aborted)
        keep(context, s, {
          type: 'RUN_ERROR',
          message: `The connection to vos dropped: ${messageOf(error)}`
        })
    } finally {
      reader.releaseLock()
      end(s)
    }
  }

  /** Stop a stream: the request is aborted, so the host sees the client go. */
  const drop = (thread: string): void => {
    const s = streams.get(thread)
    if (!s) return
    streams.delete(thread)
    s.stop.abort()
    s.resume?.()
    end(s)
  }

  return registry
    .register('vos/agui/thread', async (params, context) => {
      const p = parse(threadParams, 'vos/agui/thread', params)
      const snapshot = await json(
        await request(context, '/agui/threads', {
          threadId: scoped(context, p.threadId),
          agentId: p.agentId
        })
      )
      if (typeof snapshot === 'object' && snapshot !== null && 'threadId' in snapshot)
        (snapshot as { threadId: unknown }).threadId = unscope(
          context,
          (snapshot as { threadId: unknown }).threadId
        )
      return snapshot
    })
    .register('vos/agui/run', async (params, context) => {
      const size = encoder.encode(JSON.stringify(params ?? null)).length
      if (size > MAX_RUN_BYTES)
        throw new HostError(
          INVALID_PARAMS,
          `A message may be at most ${MAX_RUN_BYTES.toLocaleString('en')} bytes`
        )
      const p = parse(runParams, 'vos/agui/run', params)
      if (!p.messages?.length && !p.resume?.length)
        throw new HostError(INVALID_PARAMS, 'vos/agui/run needs messages or resume')
      const thread = scoped(context, p.threadId)
      const open = streams.get(thread)
      if (open && !open.ended)
        throw new HostError(BUSY, 'This chat is already answering: stop it first')
      if (open) drop(thread) // the page has moved on from a finished run it never read to the end
      // a finished run the page never read to the end gives its place up first
      for (const [other, kept] of streams) {
        if (streams.size < MAX_RUNS_PER_FRAME) break
        if (kept.ended) drop(other)
      }
      if (streams.size >= MAX_RUNS_PER_FRAME)
        throw new HostError(
          BUSY,
          `${MAX_RUNS_PER_FRAME} chats are already answering in this window: stop one, or wait for one to finish`
        )
      if (context.signal.aborted) throw new HostError(FAILED, 'The page has gone')
      const stop = new AbortController()
      const s: Stream = {
        runId: p.runId,
        stop,
        kept: [],
        keptBytes: 0,
        lastSeq: 0,
        ended: false,
        waiter: null,
        resume: null
      }
      // the stream belongs to the document that started it
      const gone = (): void => {
        if (streams.get(thread) === s) drop(thread)
        else stop.abort()
      }
      context.signal.addEventListener('abort', gone, { once: true })
      streams.set(thread, s)
      let res: Response
      try {
        res = await request(
          context,
          '/agui',
          {
            threadId: thread,
            runId: p.runId,
            state: {},
            messages: p.messages ?? [],
            tools: [],
            context: [],
            forwardedProps: { agentId: p.agentId },
            ...(p.resume ? { resume: p.resume } : {})
          },
          stop.signal,
          true
        )
        if (!res.ok) throw new HostError(FAILED, await httpError(res), { status: res.status })
        if (!res.body) throw new HostError(FAILED, 'vos sent no stream')
      } catch (error) {
        if (streams.get(thread) === s) streams.delete(thread)
        context.signal.removeEventListener('abort', gone)
        stop.abort()
        throw error
      }
      void pump(context, s, res.body).finally(() =>
        context.signal.removeEventListener('abort', gone)
      )
      return {}
    })
    .register('vos/agui/events', async (params, context) => {
      const p = parse(eventsParams, 'vos/agui/events', params)
      const thread = scoped(context, p.threadId)
      const found = streams.get(thread)
      if (found?.runId !== p.runId)
        throw new HostError(
          INVALID_PARAMS,
          'That run is not streaming here: start it with vos/agui/run'
        )
      const s: Stream = found
      if (s.waiter) throw new HostError(BUSY, 'A pull of this run is already waiting')
      if (p.after > s.lastSeq)
        throw new HostError(INVALID_PARAMS, `There is no event ${p.after} yet`)
      // acknowledged: what the page has, it no longer needs kept
      const acknowledged = s.kept.findIndex((k) => k.seq > p.after)
      for (const k of s.kept.splice(0, acknowledged < 0 ? s.kept.length : acknowledged))
        s.keptBytes -= k.bytes
      if (s.keptBytes < MAX_BUFFERED_BYTES) {
        const resume = s.resume
        s.resume = null
        resume?.()
      }
      if (s.kept.length === 0 && !s.ended && !context.signal.aborted) {
        await new Promise<void>((resolve) => {
          const timer = setTimeout(done, idleMs)
          const onAbort = (): void => done()
          context.signal.addEventListener('abort', onAbort, { once: true })
          function done(): void {
            clearTimeout(timer)
            context.signal.removeEventListener('abort', onAbort)
            if (s.waiter === done) s.waiter = null
            resolve()
          }
          s.waiter = done
        })
      }
      const events: unknown[] = []
      let bytes = 0
      let last = p.after
      for (const kept of s.kept) {
        if (events.length > 0 && bytes + kept.bytes > MAX_ANSWER_BYTES) break
        events.push(kept.event)
        bytes += kept.bytes
        last = kept.seq
      }
      // a finished run stays (acknowledged events released) until its place is needed, so a lost last answer can be asked again
      return { events, last, done: (s.ended || context.signal.aborted) && last === s.lastSeq }
    })
    .register('vos/agui/cancel', async (params, context) => {
      const p = parse(oneThread, 'vos/agui/cancel', params)
      const thread = scoped(context, p.threadId)
      drop(thread)
      const res = await request(context, `/agui/threads/${encodeURIComponent(thread)}/cancel`)
      if (!res.ok) throw new HostError(FAILED, await httpError(res), { status: res.status })
      return {}
    })
    .register('vos/agui/detach', (params, context) => {
      const p = parse(oneThread, 'vos/agui/detach', params)
      drop(scoped(context, p.threadId))
      return {}
    })
    .register('vos/agui/settings', async (params, context) => {
      const p = parse(settingsParams, 'vos/agui/settings', params)
      const thread = scoped(context, p.threadId)
      return json(
        await request(context, `/agui/threads/${encodeURIComponent(thread)}/settings`, p.change)
      )
    })
}
