/**
 * A stand-in for the Notes app's backend, for stories and unit tests: the folders and the notes kept in the page, with
 * no host. It follows the backend's rules (server/store.luau): a new board has one folder called Notes, which cannot be
 * deleted; a folder's name is refused when blank or taken, with the same sentences; a deleted folder's notes go to
 * Recently Deleted; a deleted note cannot be changed; a note with nothing in it is deleted for good; and a board lists
 * the notes by title and preview, the most recently edited first.
 */
import type { Board, Folder, Note } from 'app:rpc'
import type { NotesEvents, NotesRpc } from './notes.svelte'

export interface FakeBackend {
  readonly rpc: NotesRpc
  readonly events: NotesEvents
  /** Send an event to the window, as the backend (`notes.changed`) or the host (`host:reconnected`) does. */
  emit(name: string, data?: unknown): void
  /** Write a note as an agent's tool does, with no event: the text a later read finds. */
  rewrite(id: number, body: string): void
}

/** A note as the backend keeps it: its whole text, and where and when. */
export interface Kept {
  id: number
  folder: number
  body: string
  pinned: boolean
  edited: string
  deleted: boolean
}

export interface FakeOptions {
  /** The folders that are already there; one called Notes when left out, as on a new board. */
  folders?: readonly Folder[]
  /** The notes that are already there. */
  notes?: readonly Kept[]
  /** The backend's clock: when a note written now was edited. */
  now?: () => Date
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {
  readonly code = -32010
}

export const NOTES: Folder = { id: 1, name: 'Notes' }

/** A note for a story's or a test's starting point: in Notes, edited at `edited`, with nothing set but what is given. */
export function note(id: number, body: string, edited: Date, given: Partial<Omit<Kept, 'id' | 'body' | 'edited'>> = {}): Kept {
  return { id, folder: NOTES.id, body, pinned: false, deleted: false, edited: edited.toISOString(), ...given }
}

/** A note as a board lists it: its first two lines that say something are its title and its preview. */
export function listed(kept: Kept): Note {
  const [title = '', preview = ''] = kept.body
    .split(/[\r\n]+/)
    .map((line) => line.trim())
    .filter((line) => line !== '')
  return { id: kept.id, folder: kept.folder, title, preview, pinned: kept.pinned, edited: kept.edited, deleted: kept.deleted }
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  const now = options.now ?? ((): Date => new Date())
  let folders: Folder[] = (options.folders ?? [NOTES]).map((f) => ({ ...f }))
  let notes: Kept[] = (options.notes ?? []).map((n) => ({ ...n }))
  let nextFolder = 1 + Math.max(0, ...folders.map((f) => f.id))
  let nextNote = 1 + Math.max(0, ...notes.map((n) => n.id))
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const board = (): Board => ({
    folders: folders.map((f) => ({ ...f })),
    notes: [...notes].sort((a, b) => b.edited.localeCompare(a.edited) || b.id - a.id).map(listed)
  })
  const find = (id: number): Kept => {
    const found = notes.find((n) => n.id === id)
    if (!found) throw new Refused('There is no such note.')
    return found
  }
  const changeable = (id: number): Kept => {
    const found = find(id)
    if (found.deleted) throw new Refused('A deleted note cannot be changed: recover it first.')
    return found
  }
  const folderOf = (id: number): Folder => {
    const found = folders.find((f) => f.id === id)
    if (!found) throw new Refused('There is no such folder.')
    return found
  }

  const rpc: NotesRpc = {
    board: async () => board(),
    addFolder: async ({ name }) => {
      const named = name.trim()
      if (named === '') throw new Refused('A folder needs a name.')
      if (folders.some((f) => f.name.toLowerCase() === named.toLowerCase())) throw new Refused(`There is already a folder called ${named}.`)
      folders = [...folders, { id: nextFolder++, name: named }]
      return board()
    },
    removeFolder: async ({ folder }) => {
      folderOf(folder)
      const first = folders[0]
      if (!first || first.id === folder) throw new Refused('The first folder cannot be deleted.')
      notes = notes.filter((n) => n.folder !== folder || n.body.trim() !== '')
      for (const kept of notes) if (kept.folder === folder) Object.assign(kept, { folder: first.id, pinned: false, deleted: true })
      folders = folders.filter((f) => f.id !== folder)
      return board()
    },
    create: async ({ folder }) => {
      folderOf(folder)
      notes = [...notes, { id: nextNote++, folder, body: '', pinned: false, edited: now().toISOString(), deleted: false }]
      return board()
    },
    read: async ({ note: id }) => ({ note: id, body: find(id).body }),
    write: async ({ note: id, body }) => {
      const found = changeable(id)
      if (found.body !== body) Object.assign(found, { body, edited: now().toISOString() })
      return board()
    },
    pin: async ({ note: id, pinned }) => {
      changeable(id).pinned = pinned
      return board()
    },
    move: async ({ note: id, folder }) => {
      const found = changeable(id)
      found.folder = folderOf(folder).id
      return board()
    },
    remove: async ({ note: id }) => {
      const found = find(id)
      if (found.body.trim() === '') notes = notes.filter((n) => n.id !== id)
      else Object.assign(found, { pinned: false, deleted: true })
      return board()
    },
    restore: async ({ note: id }) => {
      find(id).deleted = false
      return board()
    },
    purge: async ({ note: id }) => {
      if (!find(id).deleted) throw new Refused('Only a deleted note can be deleted for good.')
      notes = notes.filter((n) => n.id !== id)
      return board()
    },
    search: async ({ query }) => {
      const asked = query.trim().toLowerCase()
      return { notes: asked === '' ? [] : notes.filter((n) => !n.deleted && n.body.toLowerCase().includes(asked)).map((n) => n.id) }
    }
  }

  const events: NotesEvents = {
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
    rewrite: (id, body) => void Object.assign(find(id), { body, edited: now().toISOString() })
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: {
      board: refuse,
      addFolder: refuse,
      removeFolder: refuse,
      create: refuse,
      read: refuse,
      write: refuse,
      pin: refuse,
      move: refuse,
      remove: refuse,
      restore: refuse,
      purge: refuse,
      search: refuse
    },
    events: { on: () => () => {} },
    emit: () => {},
    rewrite: () => {}
  }
}
