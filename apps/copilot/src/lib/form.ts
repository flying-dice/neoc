/**
 * The forms a copilot and a skill are made and changed in, as the page holds them while they are typed in, and what the
 * backend is sent from them. A list of the form keeps a `key` for each row, so that a row taken out takes its own
 * fields with it.
 */
import { textOf, wordsOf } from './words'

type Copilot = VosApp.Copilot.Copilot
type Secret = VosApp.Copilot.Secret
type Server = VosApp.Copilot.Server
type Skill = VosApp.Copilot.Skill
type Variable = VosApp.Copilot.Variable

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

/** The form a copilot is added or changed in. `copilot` is the one being changed; without it the form adds one. */
export interface CopilotDraft {
  copilot: Copilot | null
  name: string
  dockerfile: string
  /** The agent's command, as one line (`wordsOf`). */
  command: string
  workdir: string
  skillsDir: string
  env: VariableDraft[]
  mcp: ServerDraft[]
  /** The ids of the skills it uses. */
  skills: string[]
}

/** A file of a skill besides its SKILL.md, as the form has it. */
export interface FileDraft {
  key: number
  path: string
  text: string
}

/** The form a skill is added or changed in. `skill` is the one being changed; without it the form adds one. */
export interface SkillDraft {
  skill: Skill | null
  name: string
  text: string
  files: FileDraft[]
}

let next = 1
const key = (): number => next++

/** The folders a new copilot starts with: the backend's own for a blank one. */
export const WORKDIR = '/workspace'
export const SKILLS_DIR = '/root/.claude/skills'

export const blankVariable = (): VariableDraft => ({ key: key(), name: '', value: '', secret: null })
export const blankServer = (type: ServerDraft['type']): ServerDraft => ({ key: key(), type, name: '', command: '', args: '', env: [], url: '', headers: [] })
export const blankFile = (): FileDraft => ({ key: key(), path: '', text: '' })

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

/** The form for `copilot`, or for a new one. */
export function copilotDraftOf(copilot: Copilot | null): CopilotDraft {
  return {
    copilot,
    name: copilot?.name ?? '',
    dockerfile: copilot?.dockerfile ?? '',
    command: textOf(copilot?.command ?? []),
    workdir: copilot?.workdir ?? WORKDIR,
    skillsDir: copilot?.skills_dir ?? SKILLS_DIR,
    env: (copilot?.env ?? []).map(variableDraftOf),
    mcp: (copilot?.mcp ?? []).map(serverDraftOf),
    skills: [...(copilot?.skills ?? [])]
  }
}

/** A row nothing was typed in is left out. */
const typed = (variable: VariableDraft): boolean => variable.name.trim() !== '' || variable.value !== '' || variable.secret !== null

const variablesOf = (variables: readonly VariableDraft[]): Variable[] =>
  variables.filter(typed).map((v) => (v.secret ? { name: v.name.trim(), secret: { ...v.secret } } : { name: v.name.trim(), value: v.value }))

const serverOf = (server: ServerDraft): Server =>
  server.type === 'stdio'
    ? { type: 'stdio', name: server.name.trim(), command: server.command.trim(), args: wordsOf(server.args), env: variablesOf(server.env) }
    : { type: 'http', name: server.name.trim(), url: server.url.trim(), headers: variablesOf(server.headers) }

/** What the backend is sent for the form (`_add_copilot`; `_update_copilot` with the copilot's id). */
export function copilotParamsOf(draft: CopilotDraft): VosApp.Copilot.AddCopilotParams {
  return {
    name: draft.name.trim(),
    dockerfile: draft.dockerfile,
    command: wordsOf(draft.command),
    workdir: draft.workdir.trim(),
    skills_dir: draft.skillsDir.trim(),
    env: variablesOf(draft.env),
    mcp: draft.mcp.map(serverOf),
    skills: [...draft.skills]
  }
}

/** The form for `skill`, or for a new one. */
export function skillDraftOf(skill: Skill | null): SkillDraft {
  return { skill, name: skill?.name ?? '', text: skill?.text ?? '', files: (skill?.files ?? []).map((f) => ({ key: key(), path: f.path, text: f.text })) }
}

/** What the backend is sent for the form (`_add_skill`; `_update_skill` with the skill's id). */
export function skillParamsOf(draft: SkillDraft): VosApp.Copilot.AddSkillParams {
  return {
    name: draft.name.trim(),
    text: draft.text,
    files: draft.files.filter((f) => f.path.trim() !== '' || f.text !== '').map((f) => ({ path: f.path.trim(), text: f.text }))
  }
}

/** How a keychain item is told apart in a picker: its service and its account. */
export const secretKey = (secret: Secret): string => JSON.stringify([secret.service, secret.account])

/** A keychain item as a picker names it. */
export const secretLabel = (secret: Secret): string => `${secret.service} (${secret.account})`
