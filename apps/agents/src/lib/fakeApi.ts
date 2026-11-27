/**
 * A stand-in for the Agents app's backend, for stories and unit tests: the agents and chats kept in the page, with no
 * host, and an agent that answers by a small script instead of an AG-UI endpoint. What the person says picks what it
 * does: `hello` is answered `Hi there`, `slow` says one piece and then goes on until it is stopped (so a story can
 * show an answer on its way without racing it), `tool` uses a tool, `ask` stops at a question, `surface` draws an
 * A2UI surface with a button, and anything else is echoed, a piece at a time.
 */
type Agent = VosApp.Agents.Agent
type Chat = VosApp.Agents.Chat
type Conversation = VosApp.Agents.Conversation
type Item = VosApp.Agents.Item
import type { AgentsEvents, AgentsRpc } from './agents.svelte'

export const SCRIPTED: Agent = { id: 'scripted', name: 'Scripted agent', url: 'http://localhost:4000/agent', token: false }

/** The surface the scripted agent draws: a greeting bound to its data, a field that writes to it, and a button. */
export const SURFACE = {
  a2ui_operations: [
    { version: 'v0.9', createSurface: { surfaceId: 'booking', catalogId: 'basic' } },
    {
      version: 'v0.9',
      updateComponents: {
        surfaceId: 'booking',
        components: [
          { id: 'root', component: 'Card', child: 'column' },
          { id: 'column', component: 'Column', children: ['title', 'who', 'guests', 'confirm'] },
          { id: 'title', component: 'Text', variant: 'h3', text: 'Book a table' },
          { id: 'who', component: 'TextField', label: 'Name', value: { path: '/name' } },
          { id: 'guests', component: 'Text', text: { call: 'formatString', args: { value: 'A table for ${/guests}, for ${/name}.' } } },
          { id: 'confirm', component: 'Button', variant: 'primary', child: 'confirm_label', action: { event: { name: 'confirm', context: { name: { path: '/name' } } } } },
          { id: 'confirm_label', component: 'Text', text: 'Confirm' }
        ]
      }
    },
    { version: 'v0.9', updateDataModel: { surfaceId: 'booking', value: { name: 'Ada', guests: 2 } } }
  ]
}

export interface FakeBackend {
  readonly rpc: AgentsRpc
  readonly events: AgentsEvents
  /** Send an event to the window, as the host does (`host:reconnected`). */
  emit(name: string, data?: unknown): void
}

export interface FakeOptions {
  agents?: readonly Agent[]
  /** Chats that are already there, the most recently used first. */
  chats?: readonly Conversation[]
  /** Milliseconds before each thing the agent does. */
  pace?: number
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {}

const TITLE_MAX = 48
const titleOf = (text: string): string => {
  const line = (text.trim().split('\n')[0] ?? '').trimEnd()
  return line.length <= TITLE_MAX ? line || 'New chat' : `${line.slice(0, TITLE_MAX - 1)}…`
}

/** What an item is made from: its kind and whatever of the rest it has. */
export type Said = Pick<Item, 'kind'> & Partial<Omit<Item, 'id' | 'chat' | 'kind'>>

const itemOf = (chat: string, id: number, said: Said): Item => ({ id, chat, text: '', name: '', data: '', done: true, ...said })

/** One thing the scripted agent does: say a piece, add an item, or go on until stopped (`null`). */
type Step = string | Said | null

/** What the script does about a prompt. */
function script(text: string): Step[] {
  const said = text.trim().toLowerCase()
  if (said === 'hello') return ['Hi ', 'there']
  if (said === 'slow') return ['Thinking', null]
  if (said === 'tool')
    return [{ kind: 'tool', name: 'notes__add', text: '{"title":"Milk"}', data: JSON.stringify({ ours: true, result: '{"id":1}' }) }, 'Added.']
  if (said === 'ask')
    return [{ kind: 'interrupt', text: 'Send the report to everyone?', name: 'approval', data: JSON.stringify({ status: 'open' }), done: false }]
  if (said === 'surface') return [{ kind: 'activity', name: 'a2ui-surface', data: JSON.stringify(SURFACE) }]
  return ['echo:', ...text.trim().split(/\s+/).map((word) => ` ${word}`)]
}

/** A chat with what was said and done in it, for a story's or a test's starting point. */
export function conversation(id: string, title: string, said: readonly (Said | readonly [kind: 'user' | 'assistant', text: string])[]): Conversation {
  const items = said.map((entry, i) => itemOf(id, i + 1, Array.isArray(entry) ? { kind: entry[0], text: entry[1] } : (entry as Said)))
  return {
    chat: { id, title, agent: SCRIPTED.name, updated: '2026-06-09T08:41:00.000Z' },
    items,
    busy: false,
    waiting: items.some((item) => item.kind === 'interrupt' && !item.done),
    state: ''
  }
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let agents: Agent[] = (options.agents ?? [SCRIPTED]).map((a) => ({ ...a }))
  const pace = options.pace ?? 10
  // the most recently used first
  let chats: { chat: Chat; agent: string; items: Item[] }[] = (options.chats ?? []).map((c) => ({
    chat: { ...c.chat },
    agent: SCRIPTED.id,
    items: c.items.map((item) => ({ ...item }))
  }))
  // the turn in progress in each chat: how to stop it
  const running = new Map<string, () => void>()
  const listeners = new Map<string, Set<(data: unknown) => void>>()
  let nextChat = chats.length + 1
  let nextAgent = 1
  let nextItem = 1 + Math.max(0, ...chats.flatMap((c) => c.items.map((item) => item.id)))

  const emit = (name: string, data?: unknown): void => listeners.get(name)?.forEach((fn) => fn(data))

  const find = (id: string) => {
    const found = chats.find((c) => c.chat.id === id)
    if (!found) throw new Refused('There is no such chat.')
    return found
  }
  const view = (id: string): Conversation => {
    const found = find(id)
    return {
      chat: { ...found.chat },
      items: found.items.map((item) => ({ ...item })),
      busy: running.has(id),
      waiting: found.items.some((item) => item.kind === 'interrupt' && !item.done),
      state: ''
    }
  }
  const check = (name: string, url: string): void => {
    if (name.trim() === '') throw new Refused('Give the agent a name.')
    if (!/^https?:\/\/[^/]/.test(url.trim())) throw new Refused('The endpoint must be an http:// or https:// URL.')
  }

  function finish(id: string): void {
    for (const item of find(id).items) if (item.kind !== 'interrupt') item.done = true
    running.delete(id)
    emit('turn', { chat: id })
  }

  /** The agent's turn: each step of the script, a moment apart. */
  function turn(id: string, steps: Step[]): void {
    const found = find(id)
    let answer: Item | undefined
    let timer: ReturnType<typeof setTimeout> | undefined
    const next = (): void => {
      const step = steps.shift()
      if (step === undefined) return finish(id)
      if (step === null) return // going on, until stopped
      if (typeof step === 'string') {
        if (!answer) {
          answer = itemOf(id, nextItem++, { kind: 'assistant', done: false })
          found.items.push(answer)
          emit('item', { chat: id, item: { ...answer } })
        }
        answer.text += step
        emit('piece', { chat: id, item: answer.id, text: step })
      } else {
        const item = itemOf(id, nextItem++, step)
        found.items.push(item)
        emit('item', { chat: id, item: { ...item } })
      }
      timer = setTimeout(next, pace)
    }
    timer = setTimeout(next, pace)
    running.set(id, () => clearTimeout(timer))
  }

  const free = (chat: string): void => {
    if (running.has(chat)) throw new Refused('The agent is still answering.')
  }

  const rpc: AgentsRpc = {
    agents: async () => agents.map((a) => ({ ...a })),
    addAgent: async ({ name, url, token }) => {
      check(name, url)
      agents = [...agents, { id: `agent-${nextAgent++}`, name: name.trim(), url: url.trim(), token: (token ?? '') !== '' }]
      return agents.map((a) => ({ ...a }))
    },
    updateAgent: async ({ agent, name, url, token, forget }) => {
      const who = agents.find((a) => a.id === agent)
      if (!who) throw new Refused('There is no such agent.')
      check(name, url)
      Object.assign(who, { name: name.trim(), url: url.trim(), token: (token ?? '') !== '' ? true : forget === true ? false : who.token })
      for (const c of chats) if (c.agent === who.id) c.chat.agent = who.name
      return agents.map((a) => ({ ...a }))
    },
    removeAgent: async ({ agent }) => {
      agents = agents.filter((a) => a.id !== agent)
      return agents.map((a) => ({ ...a }))
    },
    chats: async () => chats.map((c) => ({ ...c.chat })),
    start: async ({ agent }) => {
      const who = agents.find((a) => a.id === agent)
      if (!who) throw new Refused('There is no such agent.')
      const id = `chat-${nextChat++}`
      chats = [{ chat: { id, title: 'New chat', agent: who.name, updated: new Date().toISOString() }, agent: who.id, items: [] }, ...chats]
      return view(id)
    },
    open: async ({ chat }) => view(chat),
    send: async ({ chat, text }) => {
      const found = find(chat)
      if (text.trim() === '') throw new Refused('Write something first.')
      free(chat)
      found.items.push(itemOf(chat, nextItem++, { kind: 'user', text: text.trim() }))
      if (found.chat.title === 'New chat') found.chat.title = titleOf(text)
      found.chat.updated = new Date().toISOString()
      chats = [found, ...chats.filter((c) => c !== found)]
      turn(chat, script(text))
      return view(chat)
    },
    answer: async ({ chat, item, decline, payload }) => {
      const found = find(chat)
      free(chat)
      const question = found.items.find((i) => i.id === item && i.kind === 'interrupt' && !i.done)
      if (!question) throw new Refused('That question is not open any more.')
      question.done = true
      question.data = JSON.stringify({ status: decline ? 'cancelled' : 'resolved' })
      turn(chat, [decline ? 'Then I will not.' : `You answered: ${String(JSON.parse(payload ?? '""'))}`])
      return view(chat)
    },
    act: async ({ chat, action }) => {
      find(chat)
      free(chat)
      const done = JSON.parse(action) as { name: string; context: Record<string, unknown> }
      turn(chat, [`You pressed ${done.name}: ${JSON.stringify(done.context)}`])
      return view(chat)
    },
    stop: async ({ chat }) => {
      find(chat)
      const stop = running.get(chat)
      if (stop) {
        stop()
        finish(chat)
      }
      return view(chat)
    },
    removeChat: async ({ chat }) => {
      running.get(chat)?.()
      running.delete(chat)
      chats = chats.filter((c) => c.chat.id !== chat)
      return chats.map((c) => ({ ...c.chat }))
    }
  }

  const events: AgentsEvents = {
    on(name, fn) {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn)
      return () => heard.delete(fn)
    }
  }

  return { rpc, events, emit }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: {
      agents: refuse,
      addAgent: refuse,
      updateAgent: refuse,
      removeAgent: refuse,
      chats: refuse,
      start: refuse,
      open: refuse,
      send: refuse,
      answer: refuse,
      act: refuse,
      stop: refuse,
      removeChat: refuse
    },
    events: { on: () => () => {} },
    emit: () => {}
  }
}
