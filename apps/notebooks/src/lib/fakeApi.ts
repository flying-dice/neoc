/**
 * A stand-in for the Notebooks app's backend, for stories and unit tests: the notebooks and the notes kept in the page,
 * with no host. It follows the backend's rules (server/store.luau, server/text.luau): a title is a note's name in its
 * notebook, whatever its capitals; a note's links, tags and first line are read from its text; renaming a note rewrites
 * the links to it; a new library has one notebook called Notebook, and the last notebook cannot be deleted.
 */
type Hit = VosApp.Notebooks.Hit
type Library = VosApp.Notebooks.Library
type Note = VosApp.Notebooks.Note
type Notebook = VosApp.Notebooks.Notebook
type Summary = VosApp.Notebooks.Summary
import type { NotebooksEvents, NotebooksRpc } from './notebooks.svelte'
import { excerptOf, keyOf, lineWith, linksOf, mentionOf, retarget, tagsOf, titleProblem } from './wiki'

export interface FakeBackend {
  readonly rpc: NotebooksRpc
  readonly events: NotebooksEvents
  /** Send an event to the window, as the backend (`notebooks_changed`) or the host (`host:reconnected`) does. */
  emit(name: string, data?: unknown): void
}

/** A note as the stand-in keeps it. */
export interface Seed {
  readonly id: number
  readonly notebook: number
  readonly title: string
  readonly body: string
  readonly pinned: boolean
  readonly updated: string
}

export interface FakeOptions {
  /** The notebooks that are already there; one called Notebook when left out, as in a new library. */
  notebooks?: readonly Notebook[]
  /** The notes that are already there. */
  notes?: readonly Seed[]
}

/** A refusal the backend answers a call with (`ctx:fail`): the code the router gives it. */
export class Refused extends Error {
  readonly code = -32010
}

export const NOTEBOOK: Notebook = { id: 1, name: 'Notebook' }

/** The moment every seeded note was last changed, unless it says otherwise; each change after is a minute later. */
const START = Date.UTC(2026, 5, 9, 8, 0)

/** A note for a story's or a test's starting point: in the first notebook, with nothing set but what is given. */
export function note(id: number, title: string, body = '', given: Partial<Omit<Seed, 'id' | 'title' | 'body'>> = {}): Seed {
  return { id, notebook: NOTEBOOK.id, title, body, pinned: false, updated: new Date(START - id * 60_000).toISOString(), ...given }
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let notebooks: Notebook[] = (options.notebooks ?? [NOTEBOOK]).map((n) => ({ ...n }))
  let notes: Seed[] = [...(options.notes ?? [])]
  let nextNotebook = 1 + Math.max(0, ...notebooks.map((n) => n.id))
  let nextNote = 1 + Math.max(0, ...notes.map((n) => n.id))
  let changes = 0
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const now = (): string => new Date(START + ++changes * 60_000).toISOString()
  const summary = (n: Seed): Summary => ({
    id: n.id,
    notebook: n.notebook,
    title: n.title,
    excerpt: excerptOf(n.body),
    tags: tagsOf(n.body),
    pinned: n.pinned,
    updated: n.updated
  })
  const named = (notebook: number, title: string): Seed | undefined => notes.find((n) => n.notebook === notebook && keyOf(n.title) === keyOf(title))
  const library = (): Library => ({
    notebooks: notebooks.map((n) => ({ ...n })),
    notes: [...notes].sort((a, b) => b.updated.localeCompare(a.updated) || b.id - a.id).map(summary),
    links: notes.flatMap((from) =>
      linksOf(from.body).flatMap((key) => {
        const to = named(from.notebook, key)
        return to && to.id !== from.id ? [{ from: from.id, to: to.id }] : []
      })
    )
  })
  const find = (id: number): Seed => {
    const found = notes.find((n) => n.id === id)
    if (!found) throw new Refused('There is no such note.')
    return found
  }
  const read = (id: number): Note => {
    const found = find(id)
    const key = keyOf(found.title)
    return {
      id: found.id,
      notebook: found.notebook,
      title: found.title,
      body: found.body,
      pinned: found.pinned,
      updated: found.updated,
      backlinks: notes
        .filter((n) => n.notebook === found.notebook && n.id !== found.id && linksOf(n.body).includes(key))
        .sort((a, b) => keyOf(a.title).localeCompare(keyOf(b.title)))
        .map((n) => ({ id: n.id, title: n.title, excerpt: mentionOf(n.body, key) }))
    }
  }
  const change = (id: number, changed: Partial<Seed>): void => {
    notes = notes.map((n) => (n.id === id ? { ...n, ...changed } : n))
  }
  const make = (notebook: number, title: string | undefined): number => {
    if (!notebooks.some((n) => n.id === notebook)) throw new Refused('There is no such notebook.')
    let titled = title?.trim() ?? 'Untitled'
    for (let count = 2; title === undefined && named(notebook, titled); count++) titled = `Untitled ${count}`
    const problem = titleProblem(titled)
    if (problem) throw new Refused(problem)
    if (named(notebook, titled)) throw new Refused(`There is already a note called ${titled}.`)
    notes = [...notes, { id: nextNote, notebook, title: titled, body: '', pinned: false, updated: now() }]
    return nextNote++
  }

  const rpc: NotebooksRpc = {
    library: async () => library(),
    read: async ({ note: id }) => read(id),
    create: async ({ notebook, title }) => {
      const id = make(notebook, title)
      return { library: library(), note: read(id) }
    },
    follow: async ({ notebook, title }) => {
      const id = named(notebook, title)?.id ?? make(notebook, title)
      return { library: library(), note: read(id) }
    },
    save: async ({ note: id, body }) => {
      find(id)
      change(id, { body, updated: now() })
      return library()
    },
    rename: async ({ note: id, title }) => {
      const found = find(id)
      const titled = title.trim()
      const problem = titleProblem(titled)
      if (problem) throw new Refused(problem)
      const other = named(found.notebook, titled)
      if (other && other.id !== id) throw new Refused(`There is already a note called ${titled}.`)
      if (titled !== found.title) {
        const key = keyOf(found.title)
        const at = now()
        notes = notes.map((n) => {
          const linking = n.notebook === found.notebook && linksOf(n.body).includes(key)
          const body = linking ? retarget(n.body, key, titled) : n.body
          if (n.id === id) return { ...n, title: titled, body, updated: at }
          return linking ? { ...n, body, updated: at } : n
        })
      }
      return { library: library(), note: read(id) }
    },
    pin: async ({ note: id, pinned }) => {
      find(id)
      change(id, { pinned })
      return library()
    },
    remove: async ({ note: id }) => {
      find(id)
      notes = notes.filter((n) => n.id !== id)
      return library()
    },
    search: async ({ notebook, query }) => {
      const words = query.trim().toLowerCase()
      if (words === '') return { hits: [] }
      const inside = library().notes.filter((n) => n.notebook === notebook)
      const hit = (n: Summary): Hit => ({ id: n.id, title: n.title, excerpt: lineWith(find(n.id).body, words) || n.excerpt })
      const titled = inside.filter((n) => n.title.toLowerCase().includes(words))
      const said = inside.filter((n) => !titled.includes(n) && find(n.id).body.toLowerCase().includes(words))
      return { hits: [...titled, ...said].map(hit) }
    },
    addNotebook: async ({ name }) => {
      const named = name.trim()
      if (named === '') throw new Refused('A notebook needs a name.')
      if (notebooks.some((n) => keyOf(n.name) === keyOf(named))) throw new Refused(`There is already a notebook called ${named}.`)
      notebooks = [...notebooks, { id: nextNotebook++, name: named }]
      return library()
    },
    removeNotebook: async ({ notebook }) => {
      if (!notebooks.some((n) => n.id === notebook)) throw new Refused('There is no such notebook.')
      if (notebooks.length <= 1) throw new Refused('The last notebook cannot be deleted.')
      notebooks = notebooks.filter((n) => n.id !== notebook)
      notes = notes.filter((n) => n.notebook !== notebook)
      return library()
    }
  }

  const events: NotebooksEvents = {
    on(name, fn) {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn)
      return () => heard.delete(fn)
    }
  }

  return { rpc, events, emit: (name, data) => listeners.get(name)?.forEach((fn) => fn(data)) }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: {
      library: refuse,
      read: refuse,
      create: refuse,
      follow: refuse,
      save: refuse,
      rename: refuse,
      pin: refuse,
      remove: refuse,
      search: refuse,
      addNotebook: refuse,
      removeNotebook: refuse
    },
    events: { on: () => () => {} },
    emit: () => {}
  }
}

/** A notebook with notes that link to each other, tagged and bookmarked: what the stories of a used library start from. */
export const SAMPLE: readonly Seed[] = [
  note(
    1,
    'Garden plan',
    '# Garden plan\n\nThe beds for this year, with what goes in each. See [[Tomatoes]] and [[Herbs|the herb bed]].\n\n- [x] Order the seeds\n- [ ] Clear the north bed\n- [ ] Build the cold frame\n\n> Sow after the last frost.\n\n#garden #plans',
    { pinned: true }
  ),
  note(2, 'Tomatoes', 'Sow indoors in **March**, plant out in *May*. They share a bed with basil: see [[Herbs]].\n\n#garden'),
  note(3, 'Herbs', 'Basil, thyme and chives by the kitchen door. Part of the [[Garden plan]].\n\n#garden #kitchen'),
  note(4, 'Reading list', '1. The Overstory\n2. Braiding Sweetgrass\n\nNotes go in [[Book notes]].\n\n#books'),
  note(5, '2026-06-09', 'Cleared half of the north bed for the [[Garden plan]]. Rain from four.')
]
