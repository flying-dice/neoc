/**
 * One chat thread as the page sees it: a plain, framework-free store over the host page's `vos/agui/*` services (the
 * host's AG-UI contract, which the page cannot reach itself: `client/src/shell/hostServices/agui.ts`).
 *
 *   const chat = new ChatSession({ threadId, agentId, transport: hostTransport(host.call) })
 *   chat.subscribe(() => render(chat.getState()))
 *   await chat.open()            // ensure the thread, load history, state and pending permission prompts
 *   await chat.send('hello')     // a run: streams AG-UI events into `items`
 *
 * It folds the AG-UI events into a transcript (`ChatItem`s), keeps the host's `ThreadState` (session settings,
 * slash commands, context usage, plan) up to date from `STATE_SNAPSHOT` / `STATE_DELTA`, turns an interrupt into a
 * permission prompt and answers it with a resume run, and cancels (stop pulling + `vos/agui/cancel`). A run's events are
 * pulled (`vos/agui/events`), each pull naming the last event it has. State is replaced, never mutated, so a Svelte wrapper
 * only has to assign `getState()` on every notification.
 */
import { EventType, type JsonPatchOperation } from '@ag-ui/core'
import { applyPatch } from './jsonPatch'
import {
  emptyThreadState,
  parseEvent,
  parseThreadState,
  permissionOf,
  snapshotSchema,
  toolEventSchema,
  TOOL_EVENT,
  uniqueBy,
  type AguiEvent,
  type Interrupt,
  type PermissionOption,
  type ResumeEntry,
  type ThreadSnapshot,
  type ThreadState,
  type ToolStatus
} from './schema'
import type { ChatTransport, RunRequest, SettingsChange } from './transport'

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

export type ChatStatus = 'connecting' | 'idle' | 'running' | 'waiting' | 'error'

export type ChatItem =
  | { kind: 'user'; id: string; text: string }
  | { kind: 'agent'; id: string; text: string; streaming: boolean }
  | { kind: 'thought'; id: string; text: string }
  | { kind: 'tool'; id: string; name: string; title: string; toolKind?: string; status: ToolStatus }
  | { kind: 'error'; id: string; text: string }
  | {
      kind: 'permission'
      id: string
      interruptId: string
      toolCallId?: string
      title: string
      options: PermissionOption[]
      /** What was chosen, once answered (the prompt stays in the transcript). */
      answer?: string
    }

export interface ChatState {
  threadId: string
  agentId: string
  status: ChatStatus
  /** The thread exists on the host and its history is loaded. */
  ready: boolean
  items: ChatItem[]
  state: ThreadState
  /** Why the thread could not be opened (a failed run is an `error` item instead). */
  error: string | null
}

export interface ChatSessionOptions {
  /** The page's own id of the chat (the host page puts the app in front of it). */
  threadId: string
  agentId: string
  /** The host page's services (a test passes a fake). */
  transport: ChatTransport
  newId?: () => string
}

/** "mcp__vos__calculator__show_on_calculator" is shown as "Calculator · show on calculator". */
export function friendlyTool(title: string): string {
  const m = /^mcp__vos__([a-z0-9-]+?)__(.+)$/.exec(title)
  if (!m) return title
  const app = m[1].replace(/[-_]/g, ' ').replace(/^\w/, (c) => c.toUpperCase())
  return `${app} · ${m[2].replace(/_/g, ' ')}`
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)
const str = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined)

function contentText(content: unknown): string {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content.map((p) => (isRecord(p) && p.type === 'text' ? (str(p.text) ?? '') : '')).join('')
}

/** A thread snapshot's AG-UI messages as transcript items. */
export function itemsFromMessages(messages: ThreadSnapshot['messages']): ChatItem[] {
  const items: ChatItem[] = []
  for (const m of messages) {
    const id = str(m.id) ?? `m${items.length}`
    const ext = isRecord(m.vos) ? m.vos : {}
    switch (m.role) {
      case 'user':
        items.push({ kind: 'user', id, text: contentText(m.content) })
        break
      case 'reasoning':
        items.push({ kind: 'thought', id, text: contentText(m.content) })
        break
      case 'assistant': {
        const text = contentText(m.content)
        if (text) items.push({ kind: 'agent', id, text, streaming: false })
        const calls = Array.isArray(m.toolCalls) ? m.toolCalls : []
        for (const call of calls) {
          if (!isRecord(call)) continue
          const fn = isRecord(call.function) ? call.function : {}
          const name = str(ext.title) ?? str(fn.name) ?? 'tool'
          const status = toolEventSchema.shape.status.safeParse(ext.status)
          items.push({
            kind: 'tool',
            id: str(call.id) ?? `${id}-tool`,
            name,
            title: friendlyTool(name),
            toolKind: str(ext.kind),
            status: status.success ? status.data : 'completed'
          })
        }
        break
      }
      case 'system':
        if (ext.error === true) items.push({ kind: 'error', id, text: contentText(m.content) })
        break
      default:
        break // `tool` results are summarised by the tool item
    }
  }
  return withUniqueIds(items)
}

/** Every item gets an id of its own: a transcript keyed by id cannot show two with the same one. */
function withUniqueIds(items: ChatItem[]): ChatItem[] {
  const seen = new Set<string>()
  return items.map((item) => {
    let id = item.id
    for (let n = 2; seen.has(id); n++) id = `${item.id}#${n}`
    seen.add(id)
    return id === item.id ? item : { ...item, id }
  })
}

const permissionItem = (interrupt: Interrupt): ChatItem => {
  const p = permissionOf(interrupt)
  return {
    kind: 'permission',
    id: `permission-${p.interruptId}`,
    interruptId: p.interruptId,
    toolCallId: p.toolCallId,
    title: friendlyTool(p.title),
    options: p.options
  }
}

export class ChatSession {
  private s: ChatState
  private readonly listeners = new Set<() => void>()
  private readonly transport: ChatTransport
  private readonly newId: () => string
  private abort: AbortController | null = null
  private opening: Promise<void> | null = null
  private disposed = false
  /** The answers given so far to the open permission prompts (a resume must cover all of them at once). */
  private readonly answers = new Map<string, PermissionOption | null>()

  constructor(o: ChatSessionOptions) {
    this.s = {
      threadId: o.threadId,
      agentId: o.agentId,
      status: 'connecting',
      ready: false,
      items: [],
      state: emptyThreadState(),
      error: null
    }
    this.transport = o.transport
    this.newId = o.newId ?? (() => crypto.randomUUID())
  }

  // ── observation ──────────────────────────────────────────────────────────────────────────────────────────

  getState(): ChatState {
    return this.s
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  private set(patch: Partial<ChatState>): void {
    if (this.disposed) return
    this.s = { ...this.s, ...patch }
    for (const l of [...this.listeners]) l()
  }

  private items(update: (items: ChatItem[]) => ChatItem[]): void {
    this.set({ items: update(this.s.items) })
  }

  /** Replace the item with this id (the newest one, if ids repeat) using `update`. */
  private amend(id: string, update: (item: ChatItem) => ChatItem): boolean {
    const at = this.s.items.findLastIndex((i) => i.id === id)
    if (at < 0) return false
    this.items((all) => all.map((item, i) => (i === at ? update(item) : item)))
    return true
  }

  private push(item: ChatItem): void {
    this.items((all) => [...all, item])
  }

  // ── opening ──────────────────────────────────────────────────────────────────────────────────────────────

  /** Ensure the thread exists on the host and load its snapshot. Safe to call again after a failure. */
  open(): Promise<void> {
    this.opening ??= this.doOpen().finally(() => (this.opening = null))
    return this.opening
  }

  private async doOpen(): Promise<void> {
    this.set({ status: 'connecting', error: null })
    try {
      this.load(snapshotSchema.parse(await this.transport.thread(this.s.threadId, this.s.agentId)))
    } catch (e) {
      this.set({ status: 'error', ready: false, error: messageOf(e) })
    }
  }

  private load(snap: ThreadSnapshot): void {
    this.answers.clear()
    const waiting = snap.interrupts.length > 0
    this.set({
      ready: true,
      error: null,
      state: snap.state,
      items: [...itemsFromMessages(snap.messages), ...snap.interrupts.map(permissionItem)],
      status: waiting ? 'waiting' : snap.running ? 'running' : 'idle'
    })
  }

  // ── sending ──────────────────────────────────────────────────────────────────────────────────────────────

  /** A new user message and the run that answers it. Ignored while a run is going or a prompt is open. */
  async send(text: string): Promise<void> {
    const body = text.trim()
    if (!body) return
    if (!this.s.ready) await this.open()
    if (!this.s.ready || this.s.status === 'running' || this.s.status === 'waiting') return
    const id = this.newId()
    this.push({ kind: 'user', id, text: body })
    await this.run({
      threadId: this.s.threadId,
      runId: this.newId(),
      agentId: this.s.agentId,
      messages: [{ id, role: 'user', content: body }]
    })
  }

  /**
   * Answer a permission prompt: an option id (allow once, allow always, reject...) or `null` to dismiss it (a cancelled
   * resume entry, which the host treats as a denial). Once every open prompt is answered, one resume run continues the turn.
   */
  async respond(permissionId: string, optionId: string | null): Promise<void> {
    const item = this.s.items.find((i) => i.kind === 'permission' && i.id === permissionId)
    if (item?.kind !== 'permission' || item.answer !== undefined || this.s.status !== 'waiting')
      return
    const option = item.options.find((o) => o.optionId === optionId) ?? null
    this.answers.set(item.interruptId, option)
    this.amend(item.id, (i) => ({ ...i, answer: option?.name ?? 'Denied' }) as ChatItem)
    const open = this.s.items.filter((i) => i.kind === 'permission' && i.answer === undefined)
    if (open.length > 0) return
    const resume: ResumeEntry[] = [...this.answers].map(([interruptId, chosen]) =>
      chosen
        ? { interruptId, status: 'resolved', payload: { optionId: chosen.optionId } }
        : { interruptId, status: 'cancelled' }
    )
    this.answers.clear()
    await this.run({
      threadId: this.s.threadId,
      runId: this.newId(),
      agentId: this.s.agentId,
      resume
    })
  }

  /** Stop the turn: drop the stream and tell the host to cancel (which also resolves a pending prompt). */
  async stop(): Promise<void> {
    const was = this.s.status
    if (was !== 'running' && was !== 'waiting') return
    this.abort?.abort()
    this.abort = null
    this.answers.clear()
    this.items((all) =>
      all.map((i) =>
        i.kind === 'permission' && i.answer === undefined
          ? { ...i, answer: 'Cancelled' }
          : i.kind === 'agent' && i.streaming
            ? { ...i, streaming: false }
            : i
      )
    )
    this.set({ status: 'idle' })
    try {
      await this.transport.cancel(this.s.threadId)
    } catch (e) {
      this.push({
        kind: 'error',
        id: this.newId(),
        text: `Could not stop the agent: ${messageOf(e)}`
      })
    }
  }

  // ── session settings ─────────────────────────────────────────────────────────────────────────────────────

  async setMode(mode: string): Promise<void> {
    const before = this.s.state
    const modes = before.session.modes
    if (modes)
      this.set({
        state: { ...before, session: { ...before.session, modes: { ...modes, current: mode } } }
      })
    await this.change({ mode }, before)
  }

  async setConfig(id: string, value: string | boolean): Promise<void> {
    const before = this.s.state
    const config = before.session.config.map((c) =>
      c.id === id ? ({ ...c, current: value } as typeof c) : c
    )
    this.set({ state: { ...before, session: { ...before.session, config } } })
    await this.change({ config: { id, value } }, before)
  }

  private async change(body: SettingsChange, before: ThreadState): Promise<void> {
    try {
      this.set({ state: parseThreadState(await this.transport.settings(this.s.threadId, body)) })
    } catch (e) {
      this.set({ state: before })
      this.push({ kind: 'error', id: this.newId(), text: messageOf(e) })
    }
  }

  // ── runs ─────────────────────────────────────────────────────────────────────────────────────────────────

  private async run(input: RunRequest): Promise<void> {
    const ctl = new AbortController()
    this.abort = ctl
    this.set({ status: 'running' })
    try {
      await this.transport.run(input)
      // pull the run's events until it ends: each pull names the last event it has
      let after = 0
      while (!ctl.signal.aborted) {
        const batch = await this.transport.events(input.threadId, input.runId, after)
        if (ctl.signal.aborted) break
        for (const event of batch.events) this.apply(parseEvent(event))
        after = batch.last
        if (batch.done) break
      }
      if (this.s.status === 'running' && !ctl.signal.aborted)
        this.fail('The connection closed before the run finished.')
    } catch (e) {
      if (!ctl.signal.aborted) this.fail(messageOf(e))
    } finally {
      if (this.abort === ctl) this.abort = null
    }
  }

  private fail(text: string): void {
    this.items((all) => [
      ...all.map((i) => (i.kind === 'agent' && i.streaming ? { ...i, streaming: false } : i)),
      { kind: 'error' as const, id: this.newId(), text }
    ])
    this.set({ status: 'error' })
  }

  private apply(e: AguiEvent): void {
    switch (e.type) {
      case EventType.RUN_STARTED:
        this.set({ status: 'running' })
        return
      case EventType.RUN_FINISHED:
        this.finish(e.outcome)
        return
      case EventType.RUN_ERROR:
        this.fail(e.message)
        return
      case EventType.TEXT_MESSAGE_START:
        if (e.role !== 'user')
          this.push({ kind: 'agent', id: e.messageId, text: '', streaming: true })
        return
      case EventType.TEXT_MESSAGE_CONTENT:
        this.amend(e.messageId, (i) => (i.kind === 'agent' ? { ...i, text: i.text + e.delta } : i))
        return
      case EventType.TEXT_MESSAGE_END:
        this.amend(e.messageId, (i) => (i.kind === 'agent' ? { ...i, streaming: false } : i))
        return
      case EventType.TEXT_MESSAGE_CHUNK:
        if (e.messageId && !this.s.items.some((i) => i.id === e.messageId))
          this.push({ kind: 'agent', id: e.messageId, text: '', streaming: true })
        if (e.messageId && e.delta)
          this.amend(e.messageId, (i) =>
            i.kind === 'agent' ? { ...i, text: i.text + e.delta } : i
          )
        return
      case EventType.REASONING_MESSAGE_START:
        this.push({ kind: 'thought', id: e.messageId, text: '' })
        return
      case EventType.REASONING_MESSAGE_CONTENT:
        this.amend(e.messageId, (i) =>
          i.kind === 'thought' ? { ...i, text: i.text + e.delta } : i
        )
        return
      case EventType.REASONING_MESSAGE_CHUNK:
        if (e.messageId && !this.s.items.some((i) => i.id === e.messageId))
          this.push({ kind: 'thought', id: e.messageId, text: '' })
        if (e.messageId && e.delta)
          this.amend(e.messageId, (i) =>
            i.kind === 'thought' ? { ...i, text: i.text + e.delta } : i
          )
        return
      case EventType.TOOL_CALL_START:
        if (!this.s.items.some((i) => i.kind === 'tool' && i.id === e.toolCallId))
          this.push({
            kind: 'tool',
            id: e.toolCallId,
            name: e.toolCallName,
            title: friendlyTool(e.toolCallName),
            status: 'pending'
          })
        return
      case EventType.TOOL_CALL_RESULT:
        this.amend(e.toolCallId, (i) =>
          i.kind === 'tool' && (i.status === 'pending' || i.status === 'in_progress')
            ? { ...i, status: 'completed' }
            : i
        )
        return
      case EventType.CUSTOM:
        if (e.name === TOOL_EVENT) this.tool(e.value)
        return
      case EventType.STATE_SNAPSHOT:
        this.set({ state: parseThreadState(e.snapshot) })
        return
      case EventType.STATE_DELTA:
        this.set({
          state: parseThreadState(applyPatch(this.s.state, e.delta as JsonPatchOperation[]))
        })
        return
      default:
        return // steps, args, ends, activity, raw: nothing to show
    }
  }

  private tool(value: unknown): void {
    const ev = toolEventSchema.parse(value)
    const known = this.amend(ev.toolCallId, (i) =>
      i.kind === 'tool'
        ? {
            ...i,
            status: ev.status,
            toolKind: ev.kind ?? i.toolKind,
            ...(ev.title ? { name: ev.title, title: friendlyTool(ev.title) } : {})
          }
        : i
    )
    if (!known)
      this.push({
        kind: 'tool',
        id: ev.toolCallId,
        name: ev.title ?? 'tool',
        title: friendlyTool(ev.title ?? 'tool'),
        toolKind: ev.kind ?? undefined,
        status: ev.status
      })
  }

  private finish(outcome: { type: string; interrupts?: Interrupt[] } | undefined): void {
    this.items((all) =>
      all.map((i) => (i.kind === 'agent' && i.streaming ? { ...i, streaming: false } : i))
    )
    if (outcome?.type === 'interrupt' && outcome.interrupts?.length) {
      const asked = uniqueBy((i: Interrupt) => i.id)(outcome.interrupts)
      // an interrupt already on screen (a repeat from the agent) is not asked twice
      this.items((all) => [
        ...all,
        ...asked.map(permissionItem).filter((p) => !all.some((i) => i.id === p.id))
      ])
      this.set({ status: 'waiting' })
    } else this.set({ status: 'idle' })
  }

  /** Stop streaming and release listeners. The host keeps the thread (and cancels a run whose stream drops). */
  dispose(): void {
    if (this.abort) {
      this.abort.abort()
      this.abort = null
      // the host page drops the stream, as a closed window did: the host cancels a run that is not waiting on a prompt
      this.transport.detach(this.s.threadId).catch(() => {})
    }
    this.disposed = true
    this.listeners.clear()
  }
}
