/**
 * A TypeScript stand-in for the host's `/agui` endpoint, as a `fetch` replacement: for unit tests and stories (the real
 * path runs against the Rust host). It speaks the contract of docs/architecture/host-stack.md: thread ensure and
 * snapshot, runs streamed as SSE frames, interrupts and resume runs, cancel and settings.
 *
 * What a run says is scripted: `fake.onRun = async (run) => { run.start(); run.text('Hi'); run.finish() }`. The script
 * decides the timing (`await run.wait(40)`, or `await gate` a play function releases), so a story can `waitFor` each stage.
 */
import { EventType } from '@ag-ui/core'
import type { Interrupt, ResumeEntry, RunAgentInput } from '@ag-ui/core'
import { applyPatch } from '../kit/agui/jsonPatch'
import type { FetchLike } from '../kit/agui/session'
import type { ThreadState } from '../kit/agui/schema'
import { emptyThreadState, threadStateSchema } from '../kit/agui/schema'

type Json = Record<string, unknown>

/** One request the fake host saw. */
export interface FakeCall {
  method: string
  path: string
  body: unknown
}

export interface ThreadRecord {
  threadId: string
  agentId: string
  running: boolean
  state: ThreadState
  messages: Json[]
  interrupts: Interrupt[]
}

/** The script's handle on one run (the SSE response being written). */
export class FakeRun {
  private seq = 0
  closed = false
  constructor(
    readonly input: RunAgentInput,
    readonly signal: AbortSignal,
    private readonly out: { frame(json: string): void; end(): void },
    readonly thread: ThreadRecord
  ) {}

  /** The text a user message in this run carries (empty for a resume). */
  get prompt(): string {
    const last = [...(this.input.messages ?? [])].reverse().find((m) => m.role === 'user')
    return typeof last?.content === 'string' ? last.content : ''
  }

  get resume(): ResumeEntry[] | undefined {
    return this.input.resume
  }

  id(prefix: string): string {
    return `${prefix}${++this.seq}`
  }

  send(event: Json): void {
    if (!this.closed && !this.signal.aborted) this.out.frame(JSON.stringify(event))
  }

  /** A comment frame (keep-alive): the client must ignore it. */
  ping(): void {
    if (!this.closed) this.out.frame(': keep-alive')
  }

  wait(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const t = setTimeout(resolve, ms)
      this.signal.addEventListener('abort', () => (clearTimeout(t), resolve()), { once: true })
    })
  }

  start(): void {
    this.send({
      type: EventType.RUN_STARTED,
      threadId: this.input.threadId,
      runId: this.input.runId
    })
    this.send({ type: EventType.STATE_SNAPSHOT, snapshot: this.thread.state })
  }

  /** A streamed assistant message, `chunk` characters at a time. */
  async text(
    text: string,
    o: { chunk?: number; delay?: number; id?: string } = {}
  ): Promise<string> {
    const messageId = o.id ?? this.id('msg-')
    this.send({ type: EventType.TEXT_MESSAGE_START, messageId, role: 'assistant' })
    const size = o.chunk ?? text.length
    for (let i = 0; i < text.length && !this.signal.aborted; i += size) {
      this.send({ type: EventType.TEXT_MESSAGE_CONTENT, messageId, delta: text.slice(i, i + size) })
      if (o.delay) await this.wait(o.delay)
    }
    this.send({ type: EventType.TEXT_MESSAGE_END, messageId })
    this.thread.messages.push({ id: messageId, role: 'assistant', content: text })
    return messageId
  }

  async thought(text: string): Promise<void> {
    const messageId = this.id('think-')
    this.send({ type: EventType.REASONING_START, messageId })
    this.send({ type: EventType.REASONING_MESSAGE_START, messageId, role: 'reasoning' })
    this.send({ type: EventType.REASONING_MESSAGE_CONTENT, messageId, delta: text })
    this.send({ type: EventType.REASONING_MESSAGE_END, messageId })
    this.send({ type: EventType.REASONING_END, messageId })
    this.thread.messages.push({ id: messageId, role: 'reasoning', content: text })
  }

  /** A tool call: start, status updates, and a result unless it failed. */
  async tool(
    title: string,
    o: {
      id?: string
      kind?: string
      status?: 'completed' | 'failed'
      result?: string
      delay?: number
    } = {}
  ): Promise<string> {
    const toolCallId = o.id ?? this.id('tool-')
    const status = o.status ?? 'completed'
    this.send({ type: EventType.TOOL_CALL_START, toolCallId, toolCallName: title })
    this.send({ type: EventType.TOOL_CALL_ARGS, toolCallId, delta: '{}' })
    this.send({ type: EventType.TOOL_CALL_END, toolCallId })
    const meta = { toolCallId, title, kind: o.kind ?? 'other' }
    this.send({
      type: EventType.CUSTOM,
      name: 'agentos.tool',
      value: { ...meta, status: 'in_progress' }
    })
    if (o.delay) await this.wait(o.delay)
    this.send({ type: EventType.CUSTOM, name: 'agentos.tool', value: { ...meta, status } })
    if (status === 'completed')
      this.send({
        type: EventType.TOOL_CALL_RESULT,
        messageId: this.id('res-'),
        toolCallId,
        content: o.result ?? 'ok'
      })
    this.thread.messages.push({
      id: this.id('call-'),
      role: 'assistant',
      toolCalls: [{ id: toolCallId, type: 'function', function: { name: title, arguments: '{}' } }],
      agentos: { title, kind: meta.kind, status }
    })
    return toolCallId
  }

  /** A state patch (`STATE_DELTA`); the fake host's own copy follows. */
  patch(ops: Parameters<typeof applyPatch>[1]): void {
    this.thread.state = threadStateSchema.parse(applyPatch(this.thread.state, ops))
    this.send({ type: EventType.STATE_DELTA, delta: ops })
  }

  finish(): void {
    this.thread.running = false
    this.send({
      type: EventType.RUN_FINISHED,
      threadId: this.input.threadId,
      runId: this.input.runId,
      outcome: { type: 'success' }
    })
    this.close()
  }

  /** Ask for a permission: the run ends with an interrupt (the turn stays suspended on the host). */
  interrupt(o: { title: string; id?: string; toolCallId?: string; options?: Json[] }): void {
    const interrupt: Interrupt = {
      id: o.id ?? this.id('int-'),
      reason: 'tool_call',
      message: o.title,
      toolCallId: o.toolCallId,
      metadata: {
        'agentos:options': o.options ?? [
          { optionId: 'allow', name: 'Allow once', kind: 'allow_once' },
          { optionId: 'allow-always', name: 'Always allow', kind: 'allow_always' },
          { optionId: 'reject', name: 'Deny', kind: 'reject_once' }
        ],
        'agentos:tool': { title: o.title, kind: 'execute' }
      }
    }
    this.thread.interrupts.push(interrupt)
    this.send({
      type: EventType.RUN_FINISHED,
      threadId: this.input.threadId,
      runId: this.input.runId,
      outcome: { type: 'interrupt', interrupts: [interrupt] }
    })
    this.close()
  }

  error(message: string, code = 'prompt_failed'): void {
    this.thread.running = false
    this.send({ type: EventType.RUN_ERROR, message, code })
    this.close()
  }

  close(): void {
    if (this.closed) return
    this.closed = true
    this.out.end()
  }
}

export type RunScript = (run: FakeRun) => Promise<void> | void

export interface FakeAguiOptions {
  agentId?: string
  state?: ThreadState
  /** Split every SSE frame into pieces of this many bytes (exercises chunk boundaries). */
  bytesPerChunk?: number
  /** Milliseconds of latency before every non-stream response. */
  latencyMs?: number
}

const json = (body: unknown, status = 200): Response =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

export class FakeAgui {
  readonly calls: FakeCall[] = []
  readonly threads = new Map<string, ThreadRecord>()
  /** What a run does (set by a test or story). */
  onRun: RunScript = (run) => {
    run.start()
    run.finish()
  }
  /** What a resume run does. Defaults to `onRun`. */
  onResume: RunScript | null = null
  /** Overrides: answer a `METHOD /path` yourself (return nothing to fall through). */
  readonly overrides = new Map<string, (body: unknown) => Response | undefined>()
  /** Runs started, newest last. */
  readonly runs: FakeRun[] = []
  private readonly encoder = new TextEncoder()

  constructor(readonly options: FakeAguiOptions = {}) {}

  readonly fetch: FetchLike = async (input, init) => {
    const url = new URL(input, 'http://fake')
    const method = (init?.method ?? 'GET').toUpperCase()
    const body = typeof init?.body === 'string' ? (JSON.parse(init.body) as unknown) : undefined
    const signal = init?.signal ?? undefined
    if (signal?.aborted) throw new DOMException('Aborted', 'AbortError')
    this.calls.push({ method, path: url.pathname, body })
    const override = this.overrides.get(`${method} ${url.pathname}`)?.(body)
    if (override) return override
    if (this.options.latencyMs) await new Promise((r) => setTimeout(r, this.options.latencyMs))
    return this.route(method, url.pathname, body, signal)
  }

  /** The calls to a route, in order. */
  callsTo(method: string, path: string): FakeCall[] {
    return this.calls.filter((c) => c.method === method && c.path === path)
  }

  thread(threadId: string, agentId = this.options.agentId ?? 'claude'): ThreadRecord {
    let t = this.threads.get(threadId)
    if (!t) {
      t = {
        threadId,
        agentId,
        running: false,
        state: structuredClone(this.options.state ?? emptyThreadState()),
        messages: [],
        interrupts: []
      }
      this.threads.set(threadId, t)
    }
    return t
  }

  private route(method: string, path: string, body: unknown, signal?: AbortSignal): Response {
    if (method === 'POST' && path === '/agui/threads') {
      const b = body as { threadId?: string; agentId?: string } | undefined
      if (!b?.threadId || !b.agentId)
        return json({ error: 'threadId and agentId are required' }, 422)
      return json(this.snapshot(this.thread(b.threadId, b.agentId)))
    }
    if (method === 'POST' && path === '/agui') return this.startRun(body, signal)
    const m = /^\/agui\/threads\/([^/]+)(?:\/(cancel|settings))?$/.exec(path)
    if (m) {
      const t = this.threads.get(decodeURIComponent(m[1]))
      if (!t) return json({ error: `no such thread ${m[1]}` }, 404)
      if (method === 'GET' && !m[2]) return json(this.snapshot(t))
      if (method === 'POST' && m[2] === 'cancel') {
        t.running = false
        t.interrupts = []
        return new Response(null, { status: 204 })
      }
      if (method === 'POST' && m[2] === 'settings') return this.settings(t, body as Json)
    }
    return json({ error: `no route ${method} ${path}` }, 404)
  }

  snapshot(t: ThreadRecord): Json {
    return {
      threadId: t.threadId,
      agentId: t.agentId,
      running: t.running,
      state: t.state,
      messages: t.messages,
      interrupts: t.interrupts
    }
  }

  private settings(t: ThreadRecord, body: Json): Response {
    const session = t.state.session
    const mode = body.mode as string | undefined
    const config = body.config as { id: string; value: string | boolean } | undefined
    if (mode !== undefined) {
      if (!session.modes?.available.some((a) => a.id === mode))
        return json({ error: `unknown mode ${mode}` }, 422)
      session.modes.current = mode
    } else if (config) {
      const setting = session.config.find((c) => c.id === config.id)
      if (!setting) return json({ error: `unknown option ${config.id}` }, 422)
      ;(setting as { current: string | boolean }).current = config.value
    } else return json({ error: 'nothing to change' }, 422)
    return json(t.state)
  }

  private startRun(body: unknown, signal?: AbortSignal): Response {
    const input = body as RunAgentInput | undefined
    if (!input?.threadId || !input.runId || !Array.isArray(input.messages))
      return new Response('invalid RunAgentInput', { status: 422 })
    const agentId = (input.forwardedProps as { agentId?: string } | undefined)?.agentId
    const thread = this.thread(input.threadId, agentId)
    // a resume continues the suspended turn, so it is not "a second run"
    if (thread.running && !input.resume)
      return json({ error: 'a run is already in progress on this thread' }, 409)
    thread.running = true
    const prompt = [...input.messages].reverse().find((m) => m.role === 'user')
    if (prompt && typeof prompt.content === 'string')
      thread.messages.push({ id: prompt.id, role: 'user', content: prompt.content })
    if (input.resume) thread.interrupts = []

    const chunk = this.options.bytesPerChunk
    let controller!: ReadableStreamDefaultController<Uint8Array>
    const stream = new ReadableStream<Uint8Array>({
      start: (c) => {
        controller = c
      }
    })
    const out = {
      frame: (data: string) => {
        const bytes = this.encoder.encode(`data: ${data}\n\n`.replace(/^data: :/, ':'))
        try {
          if (!chunk) controller.enqueue(bytes)
          else
            for (let i = 0; i < bytes.length; i += chunk)
              controller.enqueue(bytes.slice(i, i + chunk))
        } catch {
          // the reader is gone (aborted): nothing to write to
        }
      },
      end: () => {
        try {
          controller.close()
        } catch {
          // already closed or errored
        }
      }
    }
    const abort = new AbortController()
    signal?.addEventListener(
      'abort',
      () => {
        abort.abort()
        thread.running = false
        try {
          controller.error(new DOMException('Aborted', 'AbortError'))
        } catch {
          // already finished
        }
      },
      { once: true }
    )
    const run = new FakeRun(input, abort.signal, out, thread)
    this.runs.push(run)
    const script = input.resume ? (this.onResume ?? this.onRun) : this.onRun
    queueMicrotask(() => {
      void Promise.resolve(script(run)).catch((e: unknown) =>
        run.error(e instanceof Error ? e.message : String(e))
      )
    })
    return new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream' } })
  }
}
