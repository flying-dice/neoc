/**
 * What the window can show: every note (All Notes), the notes of one folder, or the deleted ones (Recently Deleted).
 * Which notes a view shows, and under which headings, is decided here, from the board.
 */
type Note = VosApp.Notes.Note
import { groupOf } from './dates'

export type View = { readonly kind: 'all' } | { readonly kind: 'folder'; readonly id: number } | { readonly kind: 'deleted' }

export const ALL: View = { kind: 'all' }
export const DELETED: View = { kind: 'deleted' }
export const folderView = (id: number): View => ({ kind: 'folder', id })
export const sameView = (a: View, b: View): boolean =>
  a.kind === b.kind && (a.kind !== 'folder' || b.kind !== 'folder' || a.id === b.id)

/** What a note with nothing in it is called, and what stands where a note has no second line. */
export const UNTITLED = 'New Note'
export const NO_PREVIEW = 'No additional text'

/** The notes a view shows, in the board's order (the most recently edited first). */
export function shown(notes: readonly Note[], view: View): Note[] {
  switch (view.kind) {
    case 'all':
      return notes.filter((n) => !n.deleted)
    case 'folder':
      return notes.filter((n) => !n.deleted && n.folder === view.id)
    case 'deleted':
      return notes.filter((n) => n.deleted)
  }
}

/** Some notes under one heading. */
export interface Section {
  readonly name: string
  readonly notes: readonly Note[]
}

export const PINNED = 'Pinned'

/** Notes under their headings: the pinned ones first, then by when they were edited (Today, Yesterday, ... a year). */
export function sections(notes: readonly Note[], now: Date): Section[] {
  const named = new Map<string, Note[]>()
  const under = (name: string, note: Note): void => {
    const held = named.get(name)
    if (held) held.push(note)
    else named.set(name, [note])
  }
  for (const note of notes) if (note.pinned) under(PINNED, note)
  for (const note of notes) if (!note.pinned) under(groupOf(note.edited, now), note)
  return [...named].map(([name, held]) => ({ name, notes: held }))
}
