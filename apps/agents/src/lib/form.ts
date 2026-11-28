/**
 * The form an agent is added or changed in, as the page holds it while it is typed in, and what the backend is sent
 * from it. What the agent's type does not take is kept while the form is open, and not sent. A list of the form keeps a
 * `key` for each row, so that a row taken out takes its own fields with it.
 */
import { folderFor, takes, type Kind } from './kinds'
import { textOf, wordsOf } from './words'

type Agent = VosApp.Agents.Agent
type Secret = VosApp.Agents.Secret
type Server = VosApp.Agents.Server
type Variable = VosApp.Agents.Variable

/** A name and its value as the form has them: an environment variable, or a header sent to an MCP server. */
export interface VariableDraft {
  key: number
  name: string
  /** The value written out; not sent while `secret` is chosen. */
  value: string
  /** The keychain item whose password is the value; null for a value written out. */
  secret: Secret | null
}

/** An MCP server as the form has it. What a server of the other type would have is kept, and not sent. */
export interface ServerDraft {
  key: number
  type: 'stdio' | 'http'
  name: string
  command: string
  /** Its arguments, as one line (`wordsOf`). */
  args: string
  env: VariableDraft[]
  url: string
  headers: VariableDraft[]
}

/** The form an agent is added or changed in. `agent` is the one being changed; without it the form adds one of `kind`. */
export interface AgentDraft {
  agent: Agent | null
  /** Its type: chosen before the form opens, and fixed once it is added. */
  kind: Kind
  name: string
  /** `remote`: its address. */
  url: string
  /** The folder its sessions work in: on its machine, in the container, or on this machine. */
  folder: string
  /** `remote`: a new token. Empty leaves the token the agent has, when it has one. */
  token: string
  /** `remote`: remove the token the agent has. */
  forget: boolean
  /** Its picture as it will be saved: the one it has, a new one (a `data:` URL), or empty for none. */
  avatar: string
  /** `program` and `container`: the agent's command, as one line (`wordsOf`). */
  command: string
  /** `container`: the image its chats' containers are made from. */
  image: string
  /** `container`: the folder its skills are copied into. */
  skillsDir: string
  /** `container`: the names of the Skills app's skills it uses. */
  skills: string[]
  /** `program` and `container`: its environment. */
  env: VariableDraft[]
  mcp: ServerDraft[]
}

let next = 1
const key = (): number => next++

/** The skills folder a new container agent starts with: the backend's own for a blank one. */
export const SKILLS_DIR = '/root/.claude/skills'

export const blankVariable = (): VariableDraft => ({ key: key(), name: '', value: '', secret: null })
export const blankServer = (type: ServerDraft['type']): ServerDraft => ({ key: key(), type, name: '', command: '', args: '', env: [], url: '', headers: [] })

const variableDraftOf = (variable: Variable): VariableDraft => ({
  key: key(),
  name: variable.name,
  value: variable.value ?? '',
  secret: variable.secret ? { ...variable.secret } : null
})

const serverDraftOf = (server: Server): ServerDraft => ({
  key: key(),
  type: server.type,
  name: server.name,
  command: server.command ?? '',
  args: textOf(server.args ?? []),
  env: (server.env ?? []).map(variableDraftOf),
  url: server.url ?? '',
  headers: (server.headers ?? []).map(variableDraftOf)
})

/** The form for `agent`, or for a new one of `kind`. */
export function agentDraftOf(agent: Agent | null, kind: Kind = agent?.kind ?? 'remote'): AgentDraft {
  const type = agent?.kind ?? kind
  return {
    agent,
    kind: type,
    name: agent?.name ?? '',
    url: agent?.url ?? '',
    folder: agent?.folder ?? folderFor(type),
    token: '',
    forget: false,
    avatar: agent?.avatar ?? '',
    command: textOf(agent?.command ?? []),
    image: agent?.image ?? '',
    skillsDir: agent?.skills_dir || SKILLS_DIR,
    skills: [...(agent?.skills ?? [])],
    env: (agent?.env ?? []).map(variableDraftOf),
    mcp: (agent?.mcp ?? []).map(serverDraftOf)
  }
}

/**
 * Whether the form can be saved: a name, and what its type cannot do without (an address; a command; an image; a
 * program's folder). What is wrong with them is the backend's to say.
 */
export function savable(draft: AgentDraft): boolean {
  const needs = takes(draft.kind)
  if (draft.name.trim() === '') return false
  if (needs.address) return draft.url.trim() !== ''
  if ((wordsOf(draft.command)[0] ?? '').trim() === '') return false
  if (needs.image) return draft.image.trim() !== ''
  return draft.folder.trim() !== ''
}

/** A row nothing was typed in is left out. */
const typed = (variable: VariableDraft): boolean => variable.name.trim() !== '' || variable.value !== '' || variable.secret !== null

const variablesOf = (variables: readonly VariableDraft[]): Variable[] =>
  variables.filter(typed).map((v) => (v.secret ? { name: v.name.trim(), secret: { ...v.secret } } : { name: v.name.trim(), value: v.value }))

const serverOf = (server: ServerDraft): Server =>
  server.type === 'stdio'
    ? { type: 'stdio', name: server.name.trim(), command: server.command.trim(), args: wordsOf(server.args), env: variablesOf(server.env) }
    : { type: 'http', name: server.name.trim(), url: server.url.trim(), headers: variablesOf(server.headers) }

/** What the form has of the agent's type, as the backend takes it: the rest is left out. */
function fieldsOf(draft: AgentDraft): Omit<VosApp.Agents.AddAgentParams, 'kind' | 'token' | 'avatar'> {
  const needs = takes(draft.kind)
  return {
    name: draft.name,
    ...(needs.address ? { url: draft.url } : {}),
    folder: draft.folder,
    ...(needs.command ? { command: wordsOf(draft.command), env: variablesOf(draft.env) } : {}),
    ...(needs.image ? { image: draft.image.trim(), skills_dir: draft.skillsDir.trim(), skills: [...draft.skills] } : {}),
    mcp: draft.mcp.map(serverOf)
  }
}

/** What the backend is sent to add the form's agent (`_add_agent`). */
export function addParamsOf(draft: AgentDraft): VosApp.Agents.AddAgentParams {
  const token = takes(draft.kind).address && draft.token !== '' ? { token: draft.token } : {}
  return { kind: draft.kind, ...fieldsOf(draft), ...token, ...(draft.avatar === '' ? {} : { avatar: draft.avatar }) }
}

/** What the backend is sent to change the form's agent (`_update_agent`): a picture that did not change is not sent again. */
export function updateParamsOf(draft: AgentDraft & { agent: Agent }): VosApp.Agents.UpdateAgentParams {
  const remote = takes(draft.kind).address
  const token = remote && draft.token !== '' ? { token: draft.token } : {}
  const avatar = draft.avatar === draft.agent.avatar ? {} : draft.avatar === '' ? { remove_avatar: true } : { avatar: draft.avatar }
  return { agent: draft.agent.id, ...fieldsOf(draft), ...(remote ? { forget: draft.forget } : {}), ...token, ...avatar }
}

/** How a keychain item is told apart in a picker: its service and its account. */
export const secretKey = (secret: Secret): string => JSON.stringify([secret.service, secret.account])

/** A keychain item as a picker names it. */
export const secretLabel = (secret: Secret): string => `${secret.service} (${secret.account})`
