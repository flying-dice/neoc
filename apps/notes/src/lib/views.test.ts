import { describe, expect, test } from 'bun:test'
import { listed, note } from './fakeApi'
import { folderProblem } from './folders'
import { ALL, DELETED, folderView, sameView, sections, shown } from './views'

const NOW = new Date(2026, 5, 9, 9, 41)
const NOTES = [
  note(1, 'Shopping\nMilk', new Date(2026, 5, 9, 8, 0)),
  note(2, 'Ideas', new Date(2026, 5, 8, 18, 0), { folder: 2 }),
  note(3, 'Wifi password', new Date(2026, 2, 14, 12, 0), { pinned: true }),
  note(4, 'Old plan', new Date(2026, 5, 9, 7, 0), { deleted: true })
].map(listed)
const ids = (notes: readonly { id: number }[]): number[] => notes.map((n) => n.id)

describe('what a view shows', () => {
  test('All Notes: every note that is not deleted', () => {
    expect(ids(shown(NOTES, ALL))).toEqual([1, 2, 3])
  })
  test('a folder: its own notes', () => {
    expect(ids(shown(NOTES, folderView(2)))).toEqual([2])
  })
  test('Recently Deleted: the deleted ones', () => {
    expect(ids(shown(NOTES, DELETED))).toEqual([4])
  })
  test('two views are the same when they show the same thing', () => {
    expect(sameView(folderView(2), folderView(2))).toBe(true)
    expect(sameView(folderView(2), folderView(3))).toBe(false)
    expect(sameView(ALL, DELETED)).toBe(false)
    expect(sameView(ALL, ALL)).toBe(true)
  })
})

describe('the headings of a list', () => {
  test('the pinned notes first, then by when they were edited', () => {
    expect(sections(shown(NOTES, ALL), NOW).map((s) => [s.name, ids(s.notes)])).toEqual([
      ['Pinned', [3]],
      ['Today', [1]],
      ['Yesterday', [2]]
    ])
  })
  test('no notes, no headings', () => {
    expect(sections([], NOW)).toEqual([])
  })
})

describe('a title and a preview', () => {
  test('are the first two lines that say something', () => {
    expect(listed(note(1, '\n  Shopping  \n\nMilk\nBread', NOW))).toMatchObject({ title: 'Shopping', preview: 'Milk' })
    expect(listed(note(1, 'Only a title', NOW))).toMatchObject({ title: 'Only a title', preview: '' })
    expect(listed(note(1, ' \n ', NOW))).toMatchObject({ title: '', preview: '' })
  })
})

describe('the name of a new folder', () => {
  const folders = [
    { id: 1, name: 'Notes' },
    { id: 2, name: 'Work' }
  ]
  test('is a problem when a folder has it already, whatever its capitals', () => {
    expect(folderProblem(' work ', folders)).toBe('There is already a folder called work.')
  })
  test('is no problem when it is free, or not typed yet', () => {
    expect(folderProblem('Recipes', folders)).toBeNull()
    expect(folderProblem('   ', folders)).toBeNull()
  })
})
