/**
 * What the window shows of a library: the notes of one notebook in the order the sidebar lists them, the tags they
 * have, and the notes a typed name could mean (the quick switcher). All of it is worked out from what the backend sent.
 */
type Summary = VosApp.Notebooks.Summary
import { keyOf } from './wiki'

export interface TagCount {
  readonly tag: string
  /** How many notes have it. */
  readonly count: number
}

/**
 * The notes of a notebook as the sidebar lists them: the bookmarked ones first, then the rest, each group in the order
 * the backend gave (the most recently changed first). With a tag, only the notes that have it.
 */
export function listed(notes: readonly Summary[], notebook: number | null, tag: string | null): Summary[] {
  const inside = notes.filter((n) => n.notebook === notebook && (tag === null || n.tags.includes(tag)))
  return [...inside.filter((n) => n.pinned), ...inside.filter((n) => !n.pinned)]
}

/** The tags of a notebook's notes with how many notes have each, the most used first, then by name. */
export function tagCounts(notes: readonly Summary[], notebook: number | null): TagCount[] {
  const counts = new Map<string, number>()
  for (const note of notes) if (note.notebook === notebook) for (const tag of note.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1)
  return [...counts].map(([tag, count]) => ({ tag, count })).sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag))
}

/** The note of a notebook a link names: the one with that title, whatever its capitals. */
export function named(notes: readonly Summary[], notebook: number | null, title: string): Summary | undefined {
  const key = keyOf(title)
  return notes.find((n) => n.notebook === notebook && keyOf(n.title) === key)
}

/**
 * How well a title answers what was typed: nothing when it does not. The letters typed must all be in the title, in
 * order (`mtg` finds Meeting). A title that starts with them is best, then one that has them together, then one that
 * has them apart, the closer together the better.
 */
function score(title: string, typed: string): number | null {
  const where = title.indexOf(typed)
  if (where === 0) return 0
  if (where > 0) return 1 + where / 1000
  let at = -1
  let first = -1
  for (const letter of typed) {
    at = title.indexOf(letter, at + 1)
    if (at === -1) return null
    if (first === -1) first = at
  }
  return 2 + (at - first) / 1000
}

/** The notes a typed name could mean, the best first; every note, as listed, when nothing is typed. */
export function matching(notes: readonly Summary[], typed: string): Summary[] {
  const wanted = keyOf(typed)
  if (wanted === '') return [...notes]
  return notes
    .flatMap((note, order) => {
      const scored = score(keyOf(note.title), wanted)
      return scored === null ? [] : [{ note, scored, order }]
    })
    .sort((a, b) => a.scored - b.scored || a.order - b.order)
    .map((found) => found.note)
}
