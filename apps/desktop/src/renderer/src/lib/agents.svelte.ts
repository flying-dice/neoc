import type { IconName } from '@agentos/ui'
import {
  hostBridge,
  type AgentSummary,
  type AgentUpdate,
  type ConfigValue,
  type SessionConfigOption,
  type SessionModeState
} from '@agentos/sdk/host/renderer'

/**
 * Agent conversations backed by real ACP sessions in the main process.
 * Session updates stream in through the SDK bridge and are folded into a
 * renderable transcript: text, tool calls and permission requests.
 */

export type ConversationStatus = 'idle' | 'starting' | 'running' | 'waiting' | 'error'

export type ChatItem =
  | { kind: 'user'; id: string; text: string }
  | { kind: 'agent'; id: string; text: string }
  | { kind: 'thought'; id: string; text: string }
  | {
      kind: 'tool'
      id: string
      title: string
      status: 'pending' | 'in_progress' | 'completed' | 'failed'
    }
  | {
      kind: 'permission'
      id: string
      requestId: string
      title: string
      options: { optionId: string; name: string; kind: string }[]
      /** Name of the chosen option once answered. */
      answer?: string
    }
  | { kind: 'error'; id: string; text: string }

export interface SlashCommand {
  name: string
  description: string
  hint?: string
}

export interface Conversation {
  id: string
  agentId: string
  sessionId?: string
  title: string
  status: ConversationStatus
  toolsConnected?: boolean
  items: ChatItem[]
  /** ACP session modes (e.g. Default / Plan / Accept edits). */
  modes?: SessionModeState | null
  /** ACP session settings: model, reasoning effort, … */
  configOptions?: SessionConfigOption[]
  /** Context window usage reported by the agent. */
  usage?: { used: number; size: number; cost?: { amount: number; currency: string } } | null
  /** Slash commands the agent advertises. */
  commands?: SlashCommand[]
}

/** Presentation for known agents (unknown ones get a neutral look). */
export const agentLooks: Record<string, { icon: IconName; color: string; tagline: string }> = {
  claude: {
    icon: 'sparkles',
    color: 'linear-gradient(180deg, #e8a87c, #d97757)',
    tagline: 'Claude via ACP'
  }
}
const fallbackLook = { icon: 'cpu' as IconName, color: 'var(--ios-gray)', tagline: 'ACP agent' }
export const lookOf = (agentId: string): (typeof agentLooks)[string] =>
  agentLooks[agentId] ?? fallbackLook

const uid = (): string => crypto.randomUUID()

class AgentStore {
  readonly available = !!hostBridge()
  agents = $state<AgentSummary[]>([])
  conversations = $state<Conversation[]>([])
  selectedId = $state<string | null>(null)

  constructor() {
    const host = hostBridge()
    if (!host) return
    void host.agents.list().then((list) => (this.agents = list))
    host.agents.onUpdate((u) => this.#apply(u))
    host.agents.onPermission(({ id, params }) => {
      const convo = this.#bySession(params.sessionId)
      if (!convo) return host.agents.respondPermission(id, null)
      convo.items.push({
        kind: 'permission',
        id: uid(),
        requestId: id,
        title: params.toolCall.title ?? 'Run a tool',
        options: params.options.map((o) => ({ optionId: o.optionId, name: o.name, kind: o.kind }))
      })
      convo.status = 'waiting'
    })
    host.agents.onExit(({ agentId, code }) => {
      for (const convo of this.conversations.filter((c) => c.agentId === agentId)) {
        convo.sessionId = undefined
        if (convo.status !== 'idle') {
          convo.status = 'error'
          convo.items.push({
            kind: 'error',
            id: uid(),
            text: `Agent exited (code ${code ?? 'unknown'})`
          })
        }
      }
    })
  }

  get selected(): Conversation | undefined {
    return this.conversations.find((c) => c.id === this.selectedId)
  }

  /** Conversations needing the user (pending permission). */
  get waitingCount(): number {
    return this.conversations.filter((c) => c.status === 'waiting').length
  }

  newConversation(agentId = this.agents[0]?.id ?? 'claude'): Conversation {
    const convo: Conversation = {
      id: uid(),
      agentId,
      title: 'New Chat',
      status: 'idle',
      items: [],
      modes: null,
      configOptions: [],
      usage: null,
      commands: []
    }
    this.conversations.unshift(convo)
    this.selectedId = convo.id
    const live = this.conversations[0]
    // Start the session up front so model / effort / mode can be chosen before the first message.
    void this.#ensureSession(live).catch((error) => this.#fail(live, error))
    return live
  }

  async setMode(modeId: string): Promise<void> {
    const convo = this.selected
    const host = hostBridge()
    if (!convo?.sessionId || !convo.modes || !host) return
    const previous = convo.modes.currentModeId
    convo.modes.currentModeId = modeId
    try {
      await host.agents.setMode(convo.agentId, convo.sessionId, modeId)
    } catch (error) {
      convo.modes.currentModeId = previous
      this.#fail(convo, error, false)
    }
  }

  async setConfig(configId: string, value: ConfigValue): Promise<void> {
    const convo = this.selected
    const host = hostBridge()
    if (!convo?.sessionId || !host) return
    try {
      convo.configOptions = await host.agents.setConfig(
        convo.agentId,
        convo.sessionId,
        configId,
        value
      )
    } catch (error) {
      this.#fail(convo, error, false)
    }
  }

  /** Create the ACP session for a conversation if it doesn't have one yet. */
  async #ensureSession(convo: Conversation): Promise<string> {
    if (convo.sessionId) return convo.sessionId
    const host = hostBridge()
    if (!host) throw new Error('Agents need the AgentOS desktop host')
    if (!this.#starting.has(convo.id)) {
      convo.status = 'starting'
      this.#starting.set(
        convo.id,
        host.agents
          .newSession(convo.agentId)
          .then((session) => {
            convo.sessionId = session.sessionId
            convo.toolsConnected = session.toolsConnected
            convo.modes = session.modes
            convo.configOptions = session.configOptions
            if (convo.status === 'starting') convo.status = 'idle'
            return session.sessionId
          })
          .finally(() => this.#starting.delete(convo.id))
      )
    }
    return this.#starting.get(convo.id)!
  }

  #starting = new Map<string, Promise<string>>()

  #fail(convo: Conversation, error: unknown, fatal = true): void {
    if (fatal) convo.status = 'error'
    convo.items.push({
      kind: 'error',
      id: uid(),
      text: error instanceof Error ? error.message : String(error)
    })
  }

  async send(text: string): Promise<void> {
    const host = hostBridge()
    const convo = this.selected ?? this.newConversation()
    if (!host || !text.trim() || convo.status === 'running') return

    convo.items.push({ kind: 'user', id: uid(), text })
    if (convo.title === 'New Chat') convo.title = text.length > 40 ? `${text.slice(0, 40)}…` : text

    try {
      const sessionId = await this.#ensureSession(convo)
      convo.status = 'running'
      const stop = await host.agents.prompt(convo.agentId, sessionId, text)
      convo.status = 'idle'
      if (stop === 'refusal')
        convo.items.push({ kind: 'error', id: uid(), text: 'The agent declined this request.' })
    } catch (error) {
      this.#fail(convo, error)
    }
  }

  cancel(): void {
    const convo = this.selected
    if (convo?.sessionId && convo.status === 'running') {
      void hostBridge()?.agents.cancel(convo.agentId, convo.sessionId)
    }
  }

  respond(
    convo: Conversation,
    item: Extract<ChatItem, { kind: 'permission' }>,
    optionId: string | null
  ): void {
    hostBridge()?.agents.respondPermission(item.requestId, optionId)
    item.answer = item.options.find((o) => o.optionId === optionId)?.name ?? 'Denied'
    if (!convo.items.some((i) => i.kind === 'permission' && !i.answer)) convo.status = 'running'
  }

  #bySession(sessionId: string): Conversation | undefined {
    return this.conversations.find((c) => c.sessionId === sessionId)
  }

  /** Fold one ACP session update into the transcript. */
  #apply({ notification }: AgentUpdate): void {
    const convo = this.#bySession(notification.sessionId)
    if (!convo) return
    const update = notification.update
    const last = convo.items[convo.items.length - 1]

    switch (update.sessionUpdate) {
      case 'agent_message_chunk':
      case 'agent_thought_chunk': {
        if (update.content.type !== 'text') break
        const kind = update.sessionUpdate === 'agent_message_chunk' ? 'agent' : 'thought'
        if (last?.kind === kind) last.text += update.content.text
        else convo.items.push({ kind, id: uid(), text: update.content.text })
        break
      }
      case 'current_mode_update':
        if (convo.modes) convo.modes.currentModeId = update.currentModeId
        break
      case 'config_option_update':
        convo.configOptions = update.configOptions
        break
      case 'usage_update':
        convo.usage = {
          used: update.used,
          size: update.size,
          cost: update.cost
            ? { amount: update.cost.amount, currency: update.cost.currency }
            : undefined
        }
        break
      case 'available_commands_update':
        convo.commands = update.availableCommands.map((c) => ({
          name: c.name,
          description: c.description,
          hint: c.input && 'hint' in c.input ? c.input.hint : undefined
        }))
        break
      case 'session_info_update':
        if (update.title) convo.title = update.title
        break
      case 'tool_call':
        convo.items.push({
          kind: 'tool',
          id: update.toolCallId,
          title: friendlyTool(update.title),
          status: update.status ?? 'pending'
        })
        break
      case 'tool_call_update': {
        const item = convo.items.find((i) => i.kind === 'tool' && i.id === update.toolCallId)
        if (item?.kind === 'tool') {
          if (update.status) item.status = update.status
          if (update.title) item.title = friendlyTool(update.title)
        }
        break
      }
    }
  }
}

/** "mcp__agentos__calculator__show_on_calculator" → "Calculator · show on calculator" */
function friendlyTool(title: string): string {
  const m = title.match(/^mcp__agentos__([a-z0-9_]+?)__(.+)$/)
  if (!m) return title
  const app = m[1].replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase())
  return `${app} · ${m[2].replace(/_/g, ' ')}`
}

export const agentStore = new AgentStore()
