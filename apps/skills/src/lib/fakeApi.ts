/**
 * A stand-in for the Skills app's backend, for stories and unit tests: the skills kept in the page, with no host. It
 * follows the backend's rules (server/skills.luau): a name a folder can have and no other skill's, a SKILL.md that is
 * not blank, and files at paths in the skill's folder, one at each. A skill's description is the one-line
 * `description:` of its front matter (the backend also reads quoted and folded ones).
 */
import type { SkillsEvents, SkillsRpc } from './skills.svelte'

type Skill = VosApp.Skills.Skill
type SkillFile = VosApp.Skills.SkillFile

export interface FakeBackend {
  readonly rpc: SkillsRpc
  readonly events: SkillsEvents
  /** Send an event to the window, as the host does (`host:reconnected`). */
  emit(name: string): void
  /** Another window adds a skill: this one is not told. */
  elsewhere(name: string, text: string): void
}

export interface FakeOptions {
  /** The skills there are (default: `REVIEW` and `DEPLOY`). */
  skills?: readonly Skill[]
}

/** A failure the backend answers a call with (`ctx:fail`), as the page's client rejects with it. */
class Refused extends Error {
  readonly code = -32010
}

const REVIEW_TEXT = '---\nname: review\ndescription: Review a pull request before it is merged.\n---\n\n# Review\n\nRun `scripts/check.sh` first.\n'

/** A skill with a file of its own. */
export const REVIEW: Skill = {
  id: 'review',
  name: 'review',
  description: 'Review a pull request before it is merged.',
  text: REVIEW_TEXT,
  files: [{ path: 'scripts/check.sh', text: '#!/bin/sh\nbun run lint\n' }]
}

/** A skill that is only its SKILL.md, with no front matter. */
export const DEPLOY: Skill = { id: 'deploy', name: 'deploy', description: '', text: '# Deploy\n\nShip the main branch.\n', files: [] }

const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/
const copy = <T>(value: T): T => structuredClone(value)
const descriptionOf = (text: string): string => /^---\r?\n(?:.*\r?\n)*?description:[ \t]*(.*)\r?\n/.exec(text)?.[1]?.trim() ?? ''

const badPath = (path: string): string | null => {
  if (path === '') return 'Give each file of the skill its path.'
  if (path.startsWith('/') || path.includes('\\')) return `${path} must be a path in the skill's folder, with / between its parts, like scripts/check.sh.`
  if (path.split('/').some((part) => part === '' || part === '.' || part === '..')) return `${path} must be a path in the skill's folder, with no empty part, no . and no ..`
  if (path === 'SKILL.md') return "SKILL.md is the skill's text: give the other files other paths."
  return null
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let skills: Skill[] = copy([...(options.skills ?? [REVIEW, DEPLOY])])
  let next = 1
  const listeners = new Map<string, Set<() => void>>()

  const answered = <T>(answer: () => T): Promise<T> =>
    new Promise((resolve, reject) =>
      setTimeout(() => {
        try {
          resolve(answer())
        } catch (e) {
          reject(e as Error)
        }
      }, 0)
    )
  const all = (): Skill[] => copy([...skills].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0)))
  const found = (id: string): Skill => {
    const skill = skills.find((s) => s.id === id)
    if (!skill) throw new Refused('There is no such skill.')
    return skill
  }
  const checked = (name: string, text: string, files: readonly SkillFile[] | undefined, self?: string): { name: string; files: SkillFile[] } => {
    const kept = name.trim()
    if (kept === '') throw new Refused('Give the skill a name.')
    if (kept.length > 64 || !NAME.test(kept)) throw new Refused("A skill's name is the name of its folder: letters, digits, ., _ and -, starting with a letter or a digit.")
    if (skills.some((s) => s.name === kept && s.id !== self)) throw new Refused(`There is a skill named ${kept} already.`)
    if (text.trim() === '') throw new Refused("Write the skill's SKILL.md.")
    const seen = new Set<string>()
    const list: SkillFile[] = []
    for (const file of files ?? []) {
      const path = file.path.trim()
      const wrong = badPath(path)
      if (wrong) throw new Refused(wrong)
      if (seen.has(path)) throw new Refused(`The skill has two files at ${path}.`)
      seen.add(path)
      list.push({ path, text: file.text })
    }
    return { name: kept, files: list }
  }
  const add = (name: string, text: string, files?: readonly SkillFile[]): void => {
    const kept = checked(name, text, files)
    skills = [...skills, { id: `skill-${next++}`, name: kept.name, description: descriptionOf(text), text, files: kept.files }]
  }

  const rpc: SkillsRpc = {
    skills: () => answered(all),
    addSkill: ({ name, text, files }) =>
      answered(() => {
        add(name, text, files)
        return all()
      }),
    updateSkill: ({ skill: id, name, text, files }) =>
      answered(() => {
        const skill = found(id)
        const kept = checked(name, text, files, id)
        skills = skills.map((s) => (s === skill ? { ...s, name: kept.name, description: descriptionOf(text), text, files: kept.files } : s))
        return all()
      }),
    removeSkill: ({ skill: id }) =>
      answered(() => {
        const skill = found(id)
        skills = skills.filter((s) => s !== skill)
        return all()
      })
  }

  const events: SkillsEvents = {
    on(name, fn) {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn)
      return () => heard.delete(fn)
    }
  }

  return {
    rpc,
    events,
    emit: (name) => listeners.get(name)?.forEach((fn) => fn()),
    elsewhere: (name, text) => add(name, text)
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: { skills: refuse, addSkill: refuse, updateSkill: refuse, removeSkill: refuse },
    events: { on: () => () => {} },
    emit: () => {},
    elsewhere: () => {}
  }
}
