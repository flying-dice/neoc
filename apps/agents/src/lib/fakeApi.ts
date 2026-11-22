/**
 * A stand-in for the Agents app's backend, for stories and unit tests: the chats kept in the page, with no host, and
 * an agent that answers by the same small script as the backend's own stand-in (server/agent.luau): `hello` is answered
 * `Hi there`, anything else is echoed, a piece at a time. `slow` says one piece and then goes on thinking until it is
 * stopped, so a story can show an answer on its way without racing it.
 */
import type { Agent, Chat, Conversation, Message } from 'app:rpc'
import type { AgentsEvents, AgentsRpc } from './agents.svelte'

export const SCRIPTED: Agent = { id: 'scripted', name: 'Scripted agent', role: 'Answers by script', icon: 'bolt' }

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
  /** Milliseconds before each piece of an answer. */
  pace?: number
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {}

const TITLE_MAX = 48
const titleOf = (text: string): string => {
  const line = (text.trim().split('\n')[0] ?? '').trimEnd()
  return line.length <= TITLE_MAX ? line || 'New chat' : `${line.slice(0, TITLE_MAX - 1)}…`
}

/** What the script says to a prompt; `null` at the end means it never finishes by itself. */
function script(text: string): (string | null)[] {
  const said = text.trim().toLowerCase()
  if (said === 'hello') return ['Hi ', 'there']
  if (said === 'slow') return ['Thinking', null]
  return ['echo:', ...text.trim().split(/\s+/).map((word) => ` ${word}`)]
}

/** A chat with what was said in it, for a story's or a test's starting point. */
export function conversation(id: string, title: string, said: readonly [role: Message['role'], text: string][]): Conversation {
  return {
    chat: { id, title, agent: SCRIPTED.name, updated: '2026-06-09T08:41:00.000Z' },
    messages: said.map(([role, text], i) => ({ id: i + 1, chat: id, role, text, done: true })),
    busy: false
  }
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  const agents = options.agents ?? [SCRIPTED]
  const pace = options.pace ?? 10
  // the most recently used first
  let chats: { chat: Chat; agent: string; messages: Message[] }[] = (options.chats ?? []).map((c) => ({
    chat: { ...c.chat },
    agent: SCRIPTED.id,
    messages: c.messages.map((m) => ({ ...m }))
  }))
  // the turn in progress in each chat: how to stop it
  const running = new Map<string, () => void>()
  const listeners = new Map<string, Set<(data: unknown) => void>>()
  let nextChat = chats.length + 1
  let nextMessage = 1 + Math.max(0, ...chats.flatMap((c) => c.messages.map((m) => m.id)))

  const emit = (name: string, data?: unknown): void => listeners.get(name)?.forEach((fn) => fn(data))

  const find = (id: string) => {
    const found = chats.find((c) => c.chat.id === id)
    if (!found) throw new Refused('There is no such chat.')
    return found
  }
  const view = (id: string): Conversation => {
    const found = find(id)
    return { chat: { ...found.chat }, messages: found.messages.map((m) => ({ ...m })), busy: running.has(id) }
  }

  function finish(id: string, answer: Message): void {
    answer.done = true
    running.delete(id)
    emit('turn', { chat: id })
  }

  function answerTo(id: string, text: string, answer: Message): void {
    const pieces = script(text)
    let timer: ReturnType<typeof setTimeout> | undefined
    const next = (): void => {
      const piece = pieces.shift()
      if (piece === undefined) return finish(id, answer)
      if (piece === null) return // thinking, until stopped
      answer.text += piece
      emit('piece', { chat: id, message: answer.id, text: piece })
      timer = setTimeout(next, pace)
    }
    timer = setTimeout(next, pace)
    running.set(id, () => clearTimeout(timer))
  }

  const rpc: AgentsRpc = {
    agents: async () => agents.map((a) => ({ ...a })),
    chats: async () => chats.map((c) => ({ ...c.chat })),
    start: async ({ agent }) => {
      const who = agents.find((a) => a.id === agent)
      if (!who) throw new Refused('There is no such agent.')
      const id = `chat-${nextChat++}`
      chats = [{ chat: { id, title: 'New chat', agent: who.name, updated: new Date().toISOString() }, agent: who.id, messages: [] }, ...chats]
      return view(id)
    },
    open: async ({ chat }) => view(chat),
    send: async ({ chat, text }) => {
      const found = find(chat)
      if (text.trim() === '') throw new Refused('Write something first.')
      if (running.has(chat)) throw new Refused('The agent is still answering.')
      found.messages.push({ id: nextMessage++, chat, role: 'user', text: text.trim(), done: true })
      if (found.chat.title === 'New chat') found.chat.title = titleOf(text)
      found.chat.updated = new Date().toISOString()
      chats = [found, ...chats.filter((c) => c !== found)]
      const answer: Message = { id: nextMessage++, chat, role: 'agent', text: '', done: false }
      found.messages.push(answer)
      answerTo(chat, text, answer)
      return view(chat)
    },
    stop: async ({ chat }) => {
      const found = find(chat)
      const stop = running.get(chat)
      const answer = found.messages.at(-1)
      if (stop && answer) {
        stop()
        finish(chat, answer)
      }
      return view(chat)
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
    rpc: { agents: refuse, chats: refuse, start: refuse, open: refuse, send: refuse, stop: refuse },
    events: { on: () => () => {} },
    emit: () => {}
  }
}
