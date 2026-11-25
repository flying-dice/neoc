/**
 * A stand-in for the Projects app's backend, for stories and unit tests: the projects, their issues and their pages
 * kept in the page, with no host. It follows the backend's rules (server/store.luau): a blank name or title, a key
 * that is not a key and a key another project has are refused with the same sentences; an issue takes the next number
 * of its project; a workspace lists the issues in the order they were added and the pages by title; a page goes with
 * the pages under it.
 */
import type { Issue, Page, Project, Workspace } from '@projects/rpc'
import type { ProjectsEvents, ProjectsRpc } from './projects.svelte'

export interface FakeBackend {
  readonly rpc: ProjectsRpc
  readonly events: ProjectsEvents
  /** Send an event to the window, as the backend (`projects.changed`) or the host (`host:reconnected`) does. */
  emit(name: string, data?: unknown): void
  /** Write a page behind the window's back, as an agent's tool does. */
  rewrite(page: number, body: string): void
}

export interface FakeOptions {
  projects?: readonly Project[]
  /** The issues that are already there, in the order they were added. */
  issues?: readonly Issue[]
  pages?: readonly Page[]
  /** The text of the pages that have any, by the page's id. */
  bodies?: Readonly<Record<number, string>>
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {
  readonly code = -32010
}

/** When everything in a story or a test last changed, whatever the day it is looked at. */
export const THEN = '2026-06-09T08:41:00.000Z'

export const WEBSITE: Project = { id: 1, key: 'WEB', name: 'Website', color: 'blue' }

/** An issue for a story's or a test's starting point: in Website, numbered by its id, a task to do. */
export function issue(id: number, title: string, given: Partial<Omit<Issue, 'id' | 'title'>> = {}): Issue {
  const project = given.project ?? WEBSITE.id
  const key = given.key ?? `${project === WEBSITE.id ? WEBSITE.key : `P${project}`}-${id}`
  return { id, project, key, title, description: '', type: 'task', priority: 'medium', status: 'todo', updated: THEN, ...given }
}

/** A page for a story's or a test's starting point: in Website, under no other page. */
export function page(id: number, title: string, given: Partial<Omit<Page, 'id' | 'title'>> = {}): Page {
  return { id, project: WEBSITE.id, parent: 0, title, updated: THEN, ...given }
}

const isKey = (text: string): boolean => /^[A-Z][A-Z0-9]{1,4}$/.test(text)

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let projects: Project[] = (options.projects ?? []).map((p) => ({ ...p }))
  let issues: Issue[] = (options.issues ?? []).map((i) => ({ ...i }))
  let pages: Page[] = (options.pages ?? []).map((p) => ({ ...p }))
  const bodies = new Map<number, string>(Object.entries(options.bodies ?? {}).map(([id, body]) => [Number(id), body]))
  let nextProject = 1 + Math.max(0, ...projects.map((p) => p.id))
  let nextIssue = 1 + Math.max(0, ...issues.map((i) => i.id))
  let nextPage = 1 + Math.max(0, ...pages.map((p) => p.id))
  // the number the next issue of each project takes, by the project's id
  const numbers = new Map<number, number>()
  for (const i of issues) numbers.set(i.project, Math.max(numbers.get(i.project) ?? 0, Number(i.key.split('-')[1] ?? 0)))
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const workspace = (): Workspace => ({
    projects: projects.map((p) => ({ ...p })),
    issues: issues.map((i) => ({ ...i })),
    pages: pages.map((p) => ({ ...p })).sort((a, b) => a.title.localeCompare(b.title, 'en', { sensitivity: 'base' }) || a.id - b.id)
  })
  const projectOf = (id: number): Project => {
    const found = projects.find((p) => p.id === id)
    if (!found) throw new Refused('There is no such project.')
    return found
  }
  const issueOf = (id: number): Issue => {
    const found = issues.find((i) => i.id === id)
    if (!found) throw new Refused('There is no such issue.')
    return found
  }
  const pageOf = (id: number): Page => {
    const found = pages.find((p) => p.id === id)
    if (!found) throw new Refused('There is no such page.')
    return found
  }
  /** A page's id and the ids of every page under it. */
  const family = (id: number): Set<number> => {
    const ids = new Set([id])
    for (let grew = true; grew; ) {
      grew = false
      for (const p of pages) if (ids.has(p.parent) && !ids.has(p.id)) grew = Boolean(ids.add(p.id))
    }
    return ids
  }

  const rpc: ProjectsRpc = {
    workspace: async () => workspace(),
    addProject: async ({ name, key, color }) => {
      const keyed = key.trim().toUpperCase()
      if (name.trim() === '') throw new Refused('A project needs a name.')
      if (!isKey(keyed)) throw new Refused('A key is two to five letters or digits, a letter first, like WEB.')
      if (projects.some((p) => p.key === keyed)) throw new Refused(`Another project has the key ${keyed}.`)
      projects = [...projects, { id: nextProject++, key: keyed, name: name.trim(), color }]
      return workspace()
    },
    removeProject: async ({ project }) => {
      projectOf(project)
      projects = projects.filter((p) => p.id !== project)
      issues = issues.filter((i) => i.project !== project)
      pages = pages.filter((p) => p.project !== project)
      return workspace()
    },
    addIssue: async ({ project, title, type = 'task', priority = 'medium', status = 'todo' }) => {
      if (title.trim() === '') throw new Refused('An issue needs a title.')
      const { key } = projectOf(project)
      const number = (numbers.get(project) ?? 0) + 1
      numbers.set(project, number)
      const added: Issue = { id: nextIssue++, project, key: `${key}-${number}`, title: title.trim(), description: '', type, priority, status, updated: THEN }
      issues = [...issues, added]
      return workspace()
    },
    updateIssue: async ({ issue: id, title, description, type, priority, status }) => {
      const found = issueOf(id)
      if (title !== undefined && title.trim() === '') throw new Refused('An issue needs a title.')
      found.title = title?.trim() ?? found.title
      found.description = description ?? found.description
      found.type = type ?? found.type
      found.priority = priority ?? found.priority
      found.status = status ?? found.status
      return workspace()
    },
    removeIssue: async ({ issue: id }) => {
      issueOf(id)
      issues = issues.filter((i) => i.id !== id)
      return workspace()
    },
    addPage: async ({ project, title, parent = 0 }) => {
      if (title.trim() === '') throw new Refused('A page needs a title.')
      projectOf(project)
      if (parent !== 0 && !pages.some((p) => p.id === parent && p.project === project)) throw new Refused('The page it goes under is not in the project.')
      pages = [...pages, { id: nextPage++, project, parent, title: title.trim(), updated: THEN }]
      return workspace()
    },
    readPage: async ({ page: id }) => ({ page: { ...pageOf(id) }, body: bodies.get(id) ?? '' }),
    writePage: async ({ page: id, title, body }) => {
      const found = pageOf(id)
      if (title !== undefined && title.trim() === '') throw new Refused('A page needs a title.')
      found.title = title?.trim() ?? found.title
      if (body !== undefined) bodies.set(id, body)
      return workspace()
    },
    removePage: async ({ page: id }) => {
      pageOf(id)
      const gone = family(id)
      pages = pages.filter((p) => !gone.has(p.id))
      return workspace()
    }
  }

  const events: ProjectsEvents = {
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
    emit: (name, data) => listeners.get(name)?.forEach((fn) => fn(data)),
    rewrite: (id, body) => void bodies.set(id, body)
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: {
      workspace: refuse,
      addProject: refuse,
      removeProject: refuse,
      addIssue: refuse,
      updateIssue: refuse,
      removeIssue: refuse,
      addPage: refuse,
      readPage: refuse,
      writePage: refuse,
      removePage: refuse
    },
    events: { on: () => () => {} },
    emit: () => {},
    rewrite: () => {}
  }
}
