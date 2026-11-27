import { describe, expect, test } from 'bun:test'
type Summary = VosApp.Notebooks.Summary
import { listed, matching, named, tagCounts } from './library'

const summary = (id: number, title: string, given: Partial<Summary> = {}): Summary => ({
  id,
  notebook: 1,
  title,
  excerpt: '',
  tags: [],
  pinned: false,
  updated: '2026-06-09T08:00:00.000Z',
  ...given
})

const NOTES = [
  summary(1, 'Meeting notes', { tags: ['work'] }),
  summary(2, 'Garden plan', { tags: ['garden', 'plans'], pinned: true }),
  summary(3, 'Tomatoes', { tags: ['garden'] }),
  summary(4, 'Elsewhere', { notebook: 2, tags: ['garden'] })
]

const titles = (notes: readonly Summary[]): string[] => notes.map((n) => n.title)

test('a notebook lists its own notes, the bookmarked ones first', () => {
  expect(titles(listed(NOTES, 1, null))).toEqual(['Garden plan', 'Meeting notes', 'Tomatoes'])
  expect(titles(listed(NOTES, 2, null))).toEqual(['Elsewhere'])
  expect(listed(NOTES, null, null)).toEqual([])
})

test('a tag narrows the list', () => {
  expect(titles(listed(NOTES, 1, 'garden'))).toEqual(['Garden plan', 'Tomatoes'])
})

test('the tags of a notebook, the most used first, then by name', () => {
  expect(tagCounts(NOTES, 1)).toEqual([
    { tag: 'garden', count: 2 },
    { tag: 'plans', count: 1 },
    { tag: 'work', count: 1 }
  ])
})

test('a link names the note of the notebook with that title, whatever its capitals', () => {
  expect(named(NOTES, 1, ' garden PLAN ')?.id).toBe(2)
  expect(named(NOTES, 1, 'Elsewhere')).toBeUndefined()
})

describe('what a typed name could mean', () => {
  test('nothing typed: every note, as listed', () => {
    expect(titles(matching(NOTES, '  '))).toEqual(titles(NOTES))
  })

  test('a title that starts with it is before one that has it, before one that has its letters apart', () => {
    const notes = [summary(1, 'A tomato'), summary(2, 'To my mother'), summary(3, 'Tomatoes')]
    expect(titles(matching(notes, 'tom'))).toEqual(['Tomatoes', 'A tomato', 'To my mother'])
  })

  test('letters in order find a title; letters it does not have find nothing', () => {
    expect(titles(matching(NOTES, 'mtg'))).toEqual(['Meeting notes'])
    expect(matching(NOTES, 'xyz')).toEqual([])
  })
})
