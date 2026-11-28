/**
 * A stand-in for the Agents app's backend, for stories and unit tests: the agents and chats kept in the page, with no
 * host, and an agent that answers by a small script instead of an ACP agent. What the person says picks what it does:
 * `hello` is answered `Hi there`, `slow` says one piece and then goes on until it is stopped (so a story can show an
 * answer on its way without racing it), `tool` thinks, plans and changes a file, `ask` asks for a permission and goes
 * on with the answer, and anything else is echoed, a piece at a time. Agents of every type answer the same way; Docker,
 * its images, the Skills app's skills and the keychain's items are lists the backend is given.
 */
type Agent = VosApp.Agents.Agent
type Chat = VosApp.Agents.Chat
type Conversation = VosApp.Agents.Conversation
type Engine = VosApp.Agents.Engine
type Item = VosApp.Agents.Item
type Kind = VosApp.Agents.Kind
type Secret = VosApp.Agents.Secret
type SkillSummary = VosApp.Agents.SkillSummary
import { ASKING, OPTIONS, PLAN, TOOL, itemOf, type Said } from '@vos/chat/samples'
import type { AgentsEvents, AgentsRpc } from './agents.svelte'

/** What an agent of a type that does not take them has of the other types' fields: nothing. */
const BLANK = { url: '', token: false, command: [], image: '', skills_dir: '', skills: [], env: [], mcp: [] } satisfies Partial<Agent>

/** A remote agent. */
export const SCRIPTED: Agent = { ...BLANK, id: 'scripted', name: 'Scripted agent', kind: 'remote', url: 'ws://localhost:5424/acp', folder: '/home/ada/project', avatar: '' }

/** A picture an agent can have: a sun over the sea, 24 pixels across. */
export const PICTURE =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABgAAAAYCAIAAABvFaqvAAAARUlEQVR42mOIOvGLKohh1CDqGPT/DANFBgH1YyKSDcJqCh6zhopBeEzBZdaICyNy0hFD1X+CCKifsDJiDCIKjRo0kg0CANYoHLfSsIESAAAAAElFTkSuQmCC'

/** An agent with a picture. */
export const PICTURED: Agent = { ...BLANK, id: 'writer', name: 'Writer', kind: 'remote', url: 'wss://writer.example.com/acp', token: true, folder: '/srv/writing', avatar: PICTURE }

/** A container agent: an image, an ACP agent in it, a key from the keychain, a server and a skill. */
export const CODER: Agent = {
  ...BLANK,
  id: 'coder',
  name: 'Coder',
  kind: 'container',
  folder: '/workspace',
  avatar: '',
  command: ['claude-code-acp'],
  image: 'agent-coder:1',
  skills_dir: '/root/.claude/skills',
  skills: ['review'],
  env: [{ name: 'ANTHROPIC_API_KEY', secret: { service: 'anthropic', account: 'ada' } }],
  mcp: [{ type: 'stdio', name: 'files', command: 'mcp-files', args: ['--root', '/workspace'], env: [] }]
}

/** A program agent: a command run on this machine, in a folder of it. */
export const ECHO: Agent = {
  ...BLANK,
  id: 'echo',
  name: 'Echo',
  kind: 'program',
  folder: '/home/ada/project',
  avatar: '',
  command: ['python3', '-u', 'echo_agent.py'],
  env: [{ name: 'LANG', value: 'en_GB.UTF-8' }]
}

/** The keychain's items. */
export const SECRETS: Secret[] = [
  { service: 'anthropic', account: 'ada' },
  { service: 'github', account: 'ada' }
]

/** The Skills app's skills. */
export const SKILLS: SkillSummary[] = [
  { name: 'review', description: 'Review a change before it is merged.' },
  { name: 'triage', description: 'Sort new issues by what they need.' }
]

/** Docker's images. */
export const IMAGES: string[] = ['agent-coder:1', 'debian:stable']

export const REACHABLE: Engine = { reachable: true, version: '27.3.1', host: '', problem: '' }
export const UNREACHABLE: Engine = { reachable: false, version: '', host: '', problem: 'Docker is not running on this machine, or cannot be reached.' }

/** What the Keychain app's absence is answered with. */
export const NO_KEYCHAIN = 'The Keychain app is not there: install it, or open it, to choose one of its items.'
/** What the Skills app's absence is answered with. */
export const NO_SKILLS = 'The Skills app is not there: install it, or open it, to use its skills.'

/** The folders of this machine a program agent can work in. */
const FOLDERS = ['/', '/home/ada', '/home/ada/project', '/srv/writing']

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
  /** Docker as it is when the window opens. A `connect` to an empty address or `tcp://docker:2375` reaches one. */
  engine?: Engine
  /** The keychain's items; null for a Keychain app that is not there. */
  secrets?: readonly Secret[] | null
  /** The Skills app's skills; null for a Skills app that is not there. */
  skills?: readonly SkillSummary[] | null
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {}

const TITLE_MAX = 48
const titleOf = (text: string): string => {
  const line = (text.trim().split('\n')[0] ?? '').trimEnd()
  return line.length <= TITLE_MAX ? line || 'New chat' : `${line.slice(0, TITLE_MAX - 1)}…`
}

/** One thing the scripted agent does: say a piece, add an item, or go on until stopped or answered (`null`). */
type Step = string | Said | null

/** What the script does about a prompt. */
function script(text: string): Step[] {
  const said = text.trim().toLowerCase()
  if (said === 'hello') return ['Hi ', 'there']
  if (said === 'slow') return ['Thinking', null]
  if (said === 'tool') return [{ kind: 'reasoning', text: 'The list is in list.md.' }, PLAN, TOOL, 'Added.']
  if (said === 'ask') return [ASKING, null]
  return ['echo:', ...text.trim().split(/\s+/).map((word) => ` ${word}`)]
}

/** A chat with what was said and done in it, for a story's or a test's starting point; with `agent`, or the scripted one. */
export function conversation(
  id: string,
  title: string,
  said: readonly (Said | readonly [kind: 'user' | 'assistant', text: string])[],
  agent: Agent = SCRIPTED
): Conversation {
  const items = said.map((entry, i) => itemOf(id, i + 1, Array.isArray(entry) ? { kind: entry[0], text: entry[1] } : (entry as Said)))
  return {
    chat: { id, title, agent: agent.name, agent_id: agent.id, updated: '2026-06-09T08:41:00.000Z' },
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
  let engine: Engine = structuredClone(options.engine ?? REACHABLE)
  const secrets = options.secrets === undefined ? SECRETS : options.secrets
  const skills = options.skills === undefined ? SKILLS : options.skills

  /** What an agent of `kind` keeps of what is given, as the backend checks it: refused with its reason. */
  const fieldsOf = (kind: Kind, given: VosApp.Agents.AddAgentParams | VosApp.Agents.UpdateAgentParams): Omit<Agent, 'id' | 'token' | 'avatar'> => {
    const name = given.name.trim()
    if (name === '') throw new Refused('Give the agent a name.')
    const folder = given.folder?.trim() ?? ''
    const kept = { ...BLANK, name, kind, folder, mcp: structuredClone(given.mcp ?? []) }
    if (kind === 'remote') {
      const url = given.url?.trim() ?? ''
      if (!/^wss?:\/\/[^/]/.test(url)) throw new Refused('The address must be a ws:// or wss:// URL, like ws://example.com:5424/acp.')
      if (!/^(\/|[A-Za-z]:[\\/]|\\\\)/.test(folder || '/')) throw new Refused("The folder must be an absolute path on the agent's machine, like /home/me/project.")
      return { ...kept, url, folder: folder || '/' }
    }
    const command = [...(given.command ?? [])]
    if ((command[0] ?? '').trim() === '') throw new Refused("Give the agent's command.")
    for (const v of given.env ?? []) if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(v.name)) throw new Refused(`${v.name || 'A blank name'} cannot be the name of an environment variable.`)
    const env = structuredClone(given.env ?? [])
    if (kind === 'program') {
      if (folder === '') throw new Refused('Give the folder the program works in, on this machine, like /home/me/project.')
      if (!FOLDERS.includes(folder)) throw new Refused(`There is no folder ${folder} on this machine.`)
      return { ...kept, command, env }
    }
    const image = given.image?.trim() ?? ''
    if (image === '') throw new Refused("Choose the image each chat's container is made from: build or pull it in Containers.")
    if (folder !== '' && !folder.startsWith('/')) throw new Refused('The work folder must be an absolute path in the container, like /workspace.')
    return { ...kept, command, env, image, folder: folder || '/workspace', skills_dir: given.skills_dir?.trim() || '/root/.claude/skills', skills: [...(given.skills ?? [])] }
  }
  const checkAvatar = (avatar: string): void => {
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
    addAgent: async (params) => {
      const kind = params.kind ?? 'remote'
      const kept = fieldsOf(kind, params)
      checkAvatar(params.avatar ?? '')
      agents = [...agents, { ...kept, id: `agent-${nextAgent++}`, token: kind === 'remote' && (params.token ?? '') !== '', avatar: params.avatar ?? '' }]
      return structuredClone(agents)
    },
    updateAgent: async (params) => {
      const who = agents.find((a) => a.id === params.agent)
      if (!who) throw new Refused('There is no such agent.')
      const kept = fieldsOf(who.kind, params)
      checkAvatar(params.avatar ?? '')
      Object.assign(who, kept, {
        token: who.kind === 'remote' && (params.token ?? '') !== '' ? true : params.forget === true ? false : who.token,
        avatar: (params.avatar ?? '') !== '' ? params.avatar : params.remove_avatar === true ? '' : who.avatar
      })
      for (const c of chats) if (c.chat.agent_id === who.id) c.chat.agent = who.name
      return structuredClone(agents)
    },
    removeAgent: async ({ agent }) => {
      const who = agents.find((a) => a.id === agent)
      if (!who) throw new Refused('There is no such agent.')
      // a container agent's chats go with it; the others' stay, and can be read
      if (who.kind === 'container') {
        for (const c of chats) if (c.chat.agent_id === agent) running.get(c.chat.id)?.stop()
        chats = chats.filter((c) => c.chat.agent_id !== agent)
      }
      agents = agents.filter((a) => a.id !== agent)
      return structuredClone(agents)
    },
    engine: async () => structuredClone(engine),
    connect: async ({ host }) => {
      if (host !== '' && !/^(unix|tcp|http):\/\//.test(host)) throw new Refused('The address is not one Docker can be reached at.')
      engine =
        host === '' || host === 'tcp://docker:2375' ? { ...REACHABLE, host } : { reachable: false, version: '', host, problem: `Docker cannot be reached at ${host}.` }
      return structuredClone(engine)
    },
    secrets: async () => {
      if (secrets === null) throw new Refused(NO_KEYCHAIN)
      return structuredClone([...secrets])
    },
    skills: async () => (skills === null ? { skills: [], problem: NO_SKILLS } : { skills: structuredClone([...skills]), problem: '' }),
    images: async () => (engine.reachable ? { images: [...IMAGES], problem: '' } : { images: [], problem: engine.problem }),
    chats: async () => chats.map((c) => ({ ...c.chat })),
    open: async ({ chat }) => view(chat),
    readItem: async ({ chat, item }) => {
      const found = find(chat).items.find((i) => i.id === item)
      if (!found) throw new Refused('There is no such item in that chat.')
      return { ...found }
    },
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
      engine: refuse,
      connect: refuse,
      secrets: refuse,
      skills: refuse,
      images: refuse,
      chats: refuse,
      open: refuse,
      readItem: refuse,
      send: refuse,
      answer: refuse,
      stop: refuse,
      removeChat: refuse
    },
    events: { on: () => () => {} },
    emit: () => {}
  }
}
