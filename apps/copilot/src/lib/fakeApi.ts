/**
 * A stand-in for the Copilot app's backend, for stories and unit tests: the copilots, skills and chats kept in the
 * page, with no host and no Docker, and an agent that answers by a small script instead of an ACP agent in a container.
 * What the person says picks what it does: `hello` is answered `Hi there`, `slow` says one piece and then goes on until
 * it is stopped, `tool` thinks, plans and changes a file, `ask` asks for a permission and goes on with the answer, and
 * anything else is echoed, a piece at a time. A build writes a line for each line of the Dockerfile; one with
 * `RUN false` in it fails.
 */
import { ASKING, OPTIONS, PLAN, TOOL, itemOf, type Said } from '@vos/chat/samples'
import type { CopilotEvents, CopilotRpc } from './copilot.svelte'

type Chat = VosApp.Copilot.Chat
type Conversation = VosApp.Copilot.Conversation
type Copilot = VosApp.Copilot.Copilot
type Engine = VosApp.Copilot.Engine
type Item = VosApp.Copilot.Item
type Secret = VosApp.Copilot.Secret
type Skill = VosApp.Copilot.Skill

/** A copilot for code: bun and an ACP agent, a key from the keychain, a server and a skill. */
export const CODER: Copilot = {
  id: 'coder',
  name: 'Coder',
  dockerfile: 'FROM oven/bun:1\nRUN bun add -g @zed-industries/claude-code-acp\n',
  command: ['claude-code-acp'],
  workdir: '/workspace',
  skills_dir: '/root/.claude/skills',
  env: [{ name: 'ANTHROPIC_API_KEY', secret: { service: 'anthropic', account: 'ada' } }],
  mcp: [{ type: 'stdio', name: 'files', command: 'mcp-files', args: ['--root', '/workspace'], env: [] }],
  skills: ['review'],
  image: 'vos-copilot-coder:1a2b3c',
  current: true,
  building: false,
  problem: ''
}

/** A copilot for looking after machines, not built yet. */
export const ADMIN: Copilot = {
  id: 'admin',
  name: 'Sysadmin',
  dockerfile: 'FROM debian:stable\nRUN apt-get update && apt-get install -y curl dnsutils\n',
  command: ['agent', '--acp'],
  workdir: '/workspace',
  skills_dir: '/root/.claude/skills',
  env: [{ name: 'LANG', value: 'en_GB.UTF-8' }],
  mcp: [],
  skills: [],
  image: '',
  current: false,
  building: false,
  problem: ''
}

/** A skill of the library. */
export const REVIEW: Skill = {
  id: 'review',
  name: 'review',
  text: '---\nname: review\ndescription: Review a change before it is merged.\n---\n\nRead the diff, then the tests.\n',
  files: [{ path: 'scripts/check.sh', text: '#!/bin/sh\nbun run lint\n' }]
}

/** The keychain's items. */
export const SECRETS: Secret[] = [
  { service: 'anthropic', account: 'ada' },
  { service: 'github', account: 'ada' }
]

export const REACHABLE: Engine = { reachable: true, version: '27.3.1', host: '', problem: '' }
export const UNREACHABLE: Engine = { reachable: false, version: '', host: '', problem: 'Docker is not running on this machine, or cannot be reached.' }

/** What the Keychain app's absence is answered with. */
export const NO_KEYCHAIN = 'The Keychain app is not installed, or not running.'

export interface FakeBackend {
  readonly rpc: CopilotRpc
  readonly events: CopilotEvents
  /** Send an event to the window, as the host does (`host:reconnected`). */
  emit(name: string, data?: unknown): void
}

export interface FakeOptions {
  copilots?: readonly Copilot[]
  skills?: readonly Skill[]
  /** Chats that are already there, the most recently used first. */
  chats?: readonly Conversation[]
  /** The keychain's items; null for a Keychain app that is not there. */
  secrets?: readonly Secret[] | null
  /** Docker as it is when the window opens. A `_connect` to an empty address or `tcp://docker:2375` reaches one. */
  engine?: Engine
  /** Milliseconds before each thing the agent does, and each line of a build. */
  pace?: number
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

/** A chat with a copilot, with what was said and done in it, for a story's or a test's starting point. */
export function conversation(
  id: string,
  title: string,
  copilot: Copilot,
  said: readonly (Said | readonly [kind: 'user' | 'assistant', text: string])[]
): Conversation {
  const items = said.map((entry, i) => itemOf(id, i + 1, Array.isArray(entry) ? { kind: entry[0], text: entry[1] } : (entry as Said)))
  return {
    chat: { id, title, copilot: copilot.name, copilot_id: copilot.id, updated: '2026-11-20T08:41:00.000Z' },
    items,
    busy: false,
    waiting: items.some((item) => item.kind === 'permission' && !item.done)
  }
}

const copy = <T>(value: T): T => structuredClone(value)
const hash = (text: string): string => {
  let h = 0x811c9dc5
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193)
  return (h >>> 0).toString(16).padStart(8, '0').slice(0, 6)
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let copilots: Copilot[] = copy([...(options.copilots ?? [CODER, ADMIN])])
  let skills: Skill[] = copy([...(options.skills ?? [REVIEW])])
  const secrets = options.secrets === undefined ? SECRETS : options.secrets
  let engine: Engine = copy(options.engine ?? REACHABLE)
  const pace = options.pace ?? 10
  // the most recently used first
  let chats: { chat: Chat; items: Item[] }[] = (options.chats ?? []).map((c) => ({ chat: { ...c.chat }, items: c.items.map((item) => ({ ...item })) }))
  // the turn in progress in each chat: how to stop it, and how it goes on after a pause
  const running = new Map<string, { stop: () => void; resume: (steps: Step[]) => void }>()
  const listeners = new Map<string, Set<(data: unknown) => void>>()
  let nextChat = chats.length + 1
  let nextCopilot = 1
  let nextSkill = 1
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
  const copilotOf = (id: string): Copilot => {
    const found = copilots.find((c) => c.id === id)
    if (!found) throw new Refused('There is no such copilot.')
    return found
  }
  const skillOf = (id: string): Skill => {
    const found = skills.find((s) => s.id === id)
    if (!found) throw new Refused('There is no such skill.')
    return found
  }
  const checkCopilot = (params: VosApp.Copilot.AddCopilotParams): void => {
    if (params.name.trim() === '') throw new Refused('Give the copilot a name.')
    if (params.dockerfile.trim() === '') throw new Refused('Write the Dockerfile its image is built from.')
    if ((params.command[0] ?? '').trim() === '') throw new Refused("Give the agent's command.")
    for (const folder of [params.workdir, params.skills_dir]) if (folder && !folder.startsWith('/')) throw new Refused(`${folder} is not an absolute path.`)
    for (const v of params.env ?? []) if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(v.name)) throw new Refused(`${v.name || 'A blank name'} cannot be a variable's name.`)
    for (const id of params.skills ?? []) skillOf(id)
  }
  const checkSkill = (params: VosApp.Copilot.AddSkillParams, self?: string): void => {
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(params.name)) throw new Refused('A skill is named with letters, digits, ., _ and -, starting with a letter or a digit.')
    if (skills.some((s) => s.name === params.name && s.id !== self)) throw new Refused(`There is a skill named ${params.name} already.`)
    if (params.text.trim() === '') throw new Refused('Write its SKILL.md.')
    for (const f of params.files ?? []) if (f.path === '' || f.path.startsWith('/') || f.path.split('/').includes('..')) throw new Refused(`${f.path || 'A blank path'} is not a path in the skill's folder.`)
  }
  const fill = (params: VosApp.Copilot.AddCopilotParams) => ({
    name: params.name.trim(),
    dockerfile: params.dockerfile,
    command: [...params.command],
    workdir: params.workdir || '/workspace',
    skills_dir: params.skills_dir || '/root/.claude/skills',
    env: copy(params.env ?? []),
    mcp: copy(params.mcp ?? []),
    skills: [...(params.skills ?? [])]
  })

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

  /** The build of a copilot's image: a line for each line of its Dockerfile, a moment apart, then the last line. */
  function build(copilot: Copilot): void {
    copilot.building = true
    const lines = copilot.dockerfile.split('\n').filter((line) => line.trim() !== '')
    const fails = lines.some((line) => /^RUN\s+false\b/.test(line.trim()))
    const tag = `vos-copilot-${copilot.id}:${hash(copilot.dockerfile)}`
    let step = 0
    const next = (): void => {
      if (step < lines.length) {
        emit('building', { copilot: copilot.id, line: `Step ${step + 1}/${lines.length} : ${lines[step]}`, status: 'running' })
        step++
        setTimeout(next, pace)
        return
      }
      copilot.building = false
      if (fails) {
        copilot.problem = "The command '/bin/sh -c false' returned a non-zero code: 1"
        emit('building', { copilot: copilot.id, line: copilot.problem, status: 'failed' })
      } else {
        Object.assign(copilot, { image: tag, current: true, problem: '' })
        emit('building', { copilot: copilot.id, line: `Built ${tag}`, status: 'done' })
      }
    }
    setTimeout(next, pace)
  }

  const rpc: CopilotRpc = {
    engine: async () => copy(engine),
    connect: async ({ host }) => {
      if (host !== '' && !/^(unix|tcp|http):\/\//.test(host)) throw new Refused('The address is not one Docker can be reached at.')
      engine =
        host === '' || host === 'tcp://docker:2375'
          ? { ...REACHABLE, host }
          : { reachable: false, version: '', host, problem: `Docker cannot be reached at ${host}.` }
      return copy(engine)
    },
    copilots: async () => copy(copilots),
    addCopilot: async (params) => {
      checkCopilot(params)
      copilots = [...copilots, { id: `copilot-${nextCopilot++}`, ...fill(params), image: '', current: false, building: false, problem: '' }].sort((a, b) =>
        a.name.localeCompare(b.name)
      )
      return copy(copilots)
    },
    updateCopilot: async ({ copilot: id, ...params }) => {
      const copilot = copilotOf(id)
      checkCopilot(params)
      const changed = params.dockerfile !== copilot.dockerfile
      Object.assign(copilot, fill(params), changed ? { current: false } : {})
      for (const c of chats) if (c.chat.copilot_id === id) c.chat.copilot = copilot.name
      return copy(copilots)
    },
    removeCopilot: async ({ copilot: id }) => {
      const copilot = copilotOf(id)
      if (copilot.building) throw new Refused('Its image is being built. Remove it when the build is over.')
      for (const c of chats) if (c.chat.copilot_id === id) running.get(c.chat.id)?.stop()
      chats = chats.filter((c) => c.chat.copilot_id !== id)
      copilots = copilots.filter((c) => c.id !== id)
      return copy(copilots)
    },
    build: async ({ copilot: id }) => {
      const copilot = copilotOf(id)
      if (!copilot.building) build(copilot)
      return copy(copilots)
    },
    skills: async () => copy(skills),
    addSkill: async (params) => {
      checkSkill(params)
      skills = [...skills, { id: `skill-${nextSkill++}`, name: params.name, text: params.text, files: copy(params.files ?? []) }].sort((a, b) =>
        a.name.localeCompare(b.name)
      )
      return copy(skills)
    },
    updateSkill: async ({ skill: id, ...params }) => {
      const skill = skillOf(id)
      checkSkill(params, id)
      Object.assign(skill, { name: params.name, text: params.text, files: copy(params.files ?? []) })
      return copy(skills)
    },
    removeSkill: async ({ skill: id }) => {
      skillOf(id)
      skills = skills.filter((s) => s.id !== id)
      for (const copilot of copilots) copilot.skills = copilot.skills.filter((s) => s !== id)
      return copy(skills)
    },
    secrets: async () => {
      if (secrets === null) throw new Refused(NO_KEYCHAIN)
      return copy([...secrets])
    },
    chats: async () => chats.map((c) => ({ ...c.chat })),
    open: async ({ chat }) => view(chat),
    send: async ({ chat: given, copilot: id, text }) => {
      if (text.trim() === '') throw new Refused('Write something first.')
      // a new chat is made by its first message
      let chat = given ?? ''
      if (given === undefined) {
        const copilot = copilotOf(id ?? '')
        chat = `chat-${nextChat++}`
        chats = [{ chat: { id: chat, title: 'New chat', copilot: copilot.name, copilot_id: copilot.id, updated: new Date().toISOString() }, items: [] }, ...chats]
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

  const events: CopilotEvents = {
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
  const rpc: CopilotRpc = {
    engine: refuse,
    connect: refuse,
    copilots: refuse,
    addCopilot: refuse,
    updateCopilot: refuse,
    removeCopilot: refuse,
    build: refuse,
    skills: refuse,
    addSkill: refuse,
    updateSkill: refuse,
    removeSkill: refuse,
    secrets: refuse,
    chats: refuse,
    open: refuse,
    send: refuse,
    answer: refuse,
    stop: refuse,
    removeChat: refuse
  }
  return { rpc, events: { on: () => () => {} }, emit: () => {} }
}
