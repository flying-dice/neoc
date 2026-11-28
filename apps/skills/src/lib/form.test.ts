import { describe, expect, test } from 'bun:test'
import { blankFile, filesOf, skillDraftOf, skillParamsOf } from './form'
import { DEPLOY, REVIEW } from './fakeApi'

describe('the skill form', () => {
  test('starts a new skill blank', () => {
    expect(skillDraftOf(null)).toEqual({ skill: null, name: '', text: '', files: [] })
  })

  test('sends back what a skill has, as it has it', () => {
    const draft = skillDraftOf(REVIEW)
    expect(draft.skill).toBe(REVIEW)
    expect(skillParamsOf(draft)).toEqual({ name: 'review', text: REVIEW.text, files: REVIEW.files })
  })

  test('sends the name and the paths without the spaces around them, the texts as typed, and no blank file', () => {
    const draft = skillDraftOf(null)
    draft.name = ' notes '
    draft.text = '  # Notes\n'
    draft.files = [
      { ...blankFile(), path: ' a.txt ', text: ' kept ' },
      blankFile(),
      { ...blankFile(), path: '', text: 'no path yet' }
    ]
    expect(skillParamsOf(draft)).toEqual({
      name: 'notes',
      text: '  # Notes\n',
      files: [
        { path: 'a.txt', text: ' kept ' },
        { path: '', text: 'no path yet' }
      ]
    })
  })

  test('keeps a key of its own for each file', () => {
    const draft = skillDraftOf({ ...REVIEW, files: [...REVIEW.files, { path: 'b.txt', text: '' }] })
    const keys = draft.files.map((f) => f.key)
    expect(new Set(keys).size).toBe(2)
    expect(keys).not.toContain(blankFile().key)
  })
})

describe("a skill's files, as its row says them", () => {
  test('counts the files besides its SKILL.md', () => {
    expect(filesOf(DEPLOY)).toBe('SKILL.md')
    expect(filesOf(REVIEW)).toBe('SKILL.md and 1 file')
    expect(filesOf({ ...REVIEW, files: [...REVIEW.files, { path: 'b', text: '' }] })).toBe('SKILL.md and 2 files')
  })
})
