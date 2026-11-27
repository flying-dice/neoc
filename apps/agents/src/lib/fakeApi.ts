/**
 * A stand-in for the Agents app's backend, for stories and unit tests: the agents and chats kept in the page, with no
 * host, and an agent that answers by a small script instead of an ACP agent. What the person says picks what it does:
 * `hello` is answered `Hi there`, `slow` says one piece and then goes on until it is stopped (so a story can show an
 * answer on its way without racing it), `tool` thinks, plans and changes a file, `ask` asks for a permission and goes
 * on with the answer, and anything else is echoed, a piece at a time.
 */
type Agent = VosApp.Agents.Agent
type Chat = VosApp.Agents.Chat
type Conversation = VosApp.Agents.Conversation
type Item = VosApp.Agents.Item
import type { AgentsEvents, AgentsRpc } from './agents.svelte'

export const SCRIPTED: Agent = { id: 'scripted', name: 'Scripted agent', url: 'ws://localhost:5424/acp', token: false, folder: '/home/ada/project', avatar: '' }

/** A picture an agent can have: a sun over the sea, 24 pixels across. */
export const PICTURE =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABgAAAAYCAIAAABvFaqvAAAARUlEQVR42mOIOvGLKohh1CDqGPT/DANFBgH1YyKSDcJqCh6zhopBeEzBZdaICyNy0hFD1X+CCKifsDJiDCIKjRo0kg0CANYoHLfSsIESAAAAAElFTkSuQmCC'

/** An agent with a picture. */
export const PICTURED: Agent = { id: 'writer', name: 'Writer', url: 'wss://writer.example.com/acp', token: true, folder: '/srv/writing', avatar: PICTURE }

/** The options an agent offers when it asks before it changes a file. */
export const OPTIONS = [
  { option_id: 'allow', name: 'Allow', kind: 'allow_once' },
  { option_id: 'always', name: 'Always allow', kind: 'allow_always' },
  { option_id: 'reject', name: 'Reject', kind: 'reject_once' }
]

/** A change to a file, as a tool call carries it. */
export const DIFF = {
  type: 'diff',
  path: '/home/ada/project/list.md',
  patch: '--- /home/ada/project/list.md\n+++ /home/ada/project/list.md\n@@ -1,2 +1,3 @@\n # Shopping\n-Bread\n+Bread\n+Milk\n',
  new_file: false
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

/** One thing the scripted agent does: say a piece, add an item, or go on until stopped or answered (`null`). */
type Step = string | Said | null

/** A tool call of the scripted agent's: it adds milk to a list. */
export const TOOL: Said = {
  kind: 'tool',
  name: 'Add milk to list.md',
  data: JSON.stringify({ kind: 'edit', status: 'completed', content: [DIFF], locations: [{ path: DIFF.path, line: 2 }] })
}
/** The scripted agent's plan. */
export const PLAN: Said = {
  kind: 'plan',
  data: JSON.stringify({
    entries: [
      { content: 'Find the list', priority: 'high', status: 'completed' },
      { content: 'Add milk', priority: 'medium', status: 'in_progress' }
    ]
  })
}
/** The permission the scripted agent asks for. */
export const ASKING: Said = { kind: 'permission', text: 'Add milk to list.md', data: JSON.stringify({ kind: 'edit', options: OPTIONS }), done: false }

/** What the script does about a prompt. */
function script(text: string): Step[] {
  const said = text.trim().toLowerCase()
  if (said === 'hello') return ['Hi ', 'there']
  if (said === 'slow') return ['Thinking', null]
  if (said === 'tool') return [{ kind: 'reasoning', text: 'The list is in list.md.' }, PLAN, TOOL, 'Added.']
  if (said === 'ask') return [ASKING, null]
  return ['echo:', ...text.trim().split(/\s+/).map((word) => ` ${word}`)]
}

/** A chat with what was said and done in it, for a story's or a test's starting point. */
export function conversation(id: string, title: string, said: readonly (Said | readonly [kind: 'user' | 'assistant', text: string])[]): Conversation {
  const items = said.map((entry, i) => itemOf(id, i + 1, Array.isArray(entry) ? { kind: entry[0], text: entry[1] } : (entry as Said)))
  return {
    chat: { id, title, agent: SCRIPTED.name, agent_id: SCRIPTED.id, updated: '2026-06-09T08:41:00.000Z' },
    items,
    busy: false,
    waiting: items.some((item) => item.kind === 'permission' && !item.done)
  }
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let agents: Agent[] = (options.agents ?? [SCRIPTED]).map((a) => ({ ...a }))
  const pace = options.pace ?? 10
  // the most recently used first
  let chats: { chat: Chat; items: Item[] }[] = (options.chats ?? []).map((c) => ({
    chat: { ...c.chat },
    items: c.items.map((item) => ({ ...item }))
  }))
  // the turn in progress in each chat: how to stop it, and how it goes on after a pause
  const running = new Map<string, { stop: () => void; resume: (steps: Step[]) => void }>()
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
      waiting: found.items.some((item) => item.kind === 'permission' && !item.done)
    }
  }
  const check = (name: string, url: string, folder: string, avatar: string): void => {
    if (name.trim() === '') throw new Refused('Give the agent a name.')
    if (!/^wss?:\/\/[^/]/.test(url.trim())) throw new Refused('The address must be a ws:// or wss:// URL, like ws://example.com:5424/acp.')
    if (!/^(\/|[A-Za-z]:[\\/]|\\\\)/.test(folder.trim() || '/')) throw new Refused("The folder must be an absolute path on the agent's machine, like /home/me/project.")
    if (avatar.length > 200_000) throw new Refused('The picture is too big. Choose a smaller one.')
    if (avatar !== '' && !/^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(avatar)) throw new Refused('The picture must be a PNG, JPEG or WebP image.')
  }

  function finish(id: string): void {
    for (const item of find(id).items) item.done = true
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
      if (step === null) return // going on, until stopped or answered
      if (typeof step === 'string') {
        if (!answer) {
          answer = itemOf(id, nextItem++, { kind: 'assistant', done: false })
          found.items.push(answer)
          emit('item', { chat: id, item: { ...answer } })
        }
        answer.text += step
        emit('piece', { chat: id, item: answer.id, text: step })
      } else {
        answer = undefined
        const item = itemOf(id, nextItem++, step)
        found.items.push(item)
        emit('item', { chat: id, item: { ...item } })
      }
      timer = setTimeout(next, pace)
    }
    timer = setTimeout(next, pace)
    running.set(id, {
      stop: () => clearTimeout(timer),
      resume: (more) => {
        steps = more
        timer = setTimeout(next, pace)
      }
    })
  }

  const rpc: AgentsRpc = {
    agents: async () => agents.map((a) => ({ ...a })),
    addAgent: async ({ name, url, token, folder, avatar }) => {
      check(name, url, folder ?? '', avatar ?? '')
      agents = [
        ...agents,
        { id: `agent-${nextAgent++}`, name: name.trim(), url: url.trim(), token: (token ?? '') !== '', folder: folder?.trim() || '/', avatar: avatar ?? '' }
      ]
      return agents.map((a) => ({ ...a }))
    },
    updateAgent: async ({ agent, name, url, token, forget, folder, avatar, remove_avatar }) => {
      const who = agents.find((a) => a.id === agent)
      if (!who) throw new Refused('There is no such agent.')
      check(name, url, folder ?? '', avatar ?? '')
      Object.assign(who, {
        name: name.trim(),
        url: url.trim(),
        folder: folder?.trim() || '/',
        token: (token ?? '') !== '' ? true : forget === true ? false : who.token,
        avatar: (avatar ?? '') !== '' ? avatar : remove_avatar === true ? '' : who.avatar
      })
      for (const c of chats) if (c.chat.agent_id === who.id) c.chat.agent = who.name
      return agents.map((a) => ({ ...a }))
    },
    removeAgent: async ({ agent }) => {
      agents = agents.filter((a) => a.id !== agent)
      return agents.map((a) => ({ ...a }))
    },
    chats: async () => chats.map((c) => ({ ...c.chat })),
    open: async ({ chat }) => view(chat),
    send: async ({ chat: given, agent, text }) => {
      if (text.trim() === '') throw new Refused('Write something first.')
      // a new chat is made by its first message
      let chat = given ?? ''
      if (given === undefined) {
        const who = agents.find((a) => a.id === agent)
        if (!who) throw new Refused('There is no such agent.')
        chat = `chat-${nextChat++}`
        chats = [{ chat: { id: chat, title: 'New chat', agent: who.name, agent_id: who.id, updated: new Date().toISOString() }, items: [] }, ...chats]
      }
      const found = find(chat)
      if (running.has(chat)) throw new Refused('The agent is still answering.')
      found.items.push(itemOf(chat, nextItem++, { kind: 'user', text: text.trim() }))
      if (found.chat.title === 'New chat') found.chat.title = titleOf(text)
      found.chat.updated = new Date().toISOString()
      chats = [found, ...chats.filter((c) => c !== found)]
      turn(chat, script(text))
      return view(chat)
    },
    answer: async ({ chat, item, option }) => {
      const found = find(chat)
      const question = found.items.find((i) => i.id === item && i.kind === 'permission' && !i.done)
      const going = running.get(chat)
      if (!question || !going) throw new Refused('That question is not open any more.')
      const offered = OPTIONS.find((o) => o.option_id === option)
      if (!offered) throw new Refused('The agent did not offer that answer.')
      question.done = true
      question.data = JSON.stringify({ kind: 'edit', options: OPTIONS, chosen: option })
      emit('item', { chat, item: { ...question } })
      going.resume(offered.kind.startsWith('allow') ? [TOOL, 'Added.'] : ['Then I will not.'])
      return view(chat)
    },
    stop: async ({ chat }) => {
      find(chat)
      const going = running.get(chat)
      if (going) {
        going.stop()
        finish(chat)
      }
      return view(chat)
    },
    removeChat: async ({ chat }) => {
      running.get(chat)?.stop()
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
      open: refuse,
      send: refuse,
      answer: refuse,
      stop: refuse,
      removeChat: refuse
    },
    events: { on: () => () => {} },
    emit: () => {}
  }
}
