import { describe, expect, test } from 'bun:test'
import { excerptOf, keyOf, lineWith, linksOf, mentionOf, retarget, tagsOf, titleProblem, toggleTask } from './wiki'

describe('a title', () => {
  test('is compared without its capitals and outer spaces', () => {
    expect(keyOf('  Garden Plan ')).toBe('garden plan')
  })

  test('cannot be blank, too long, or have what a link is written with', () => {
    expect(titleProblem('Garden plan')).toBeNull()
    expect(titleProblem('   ')).toBe('A note needs a title.')
    expect(titleProblem('x'.repeat(121))).toBe('A title is at most 120 characters.')
    expect(titleProblem('x'.repeat(120))).toBeNull()
    for (const bad of ['a[b', 'a]b', 'a|b', 'a#b', 'a^b']) expect(titleProblem(bad)).toBe('A title cannot have [ ] | # or ^ in it.')
  })
})

describe('links', () => {
  test('are the titles in double brackets, each once, without shown text or heading', () => {
    expect(linksOf('See [[Tomatoes]] and [[Herbs|the herbs]], [[tomatoes#Sowing]] again.')).toEqual(['tomatoes', 'herbs'])
  })

  test('are not read in code', () => {
    expect(linksOf('`[[Inline]]` and\n```\n[[Fenced]]\n```\n[[Real]]')).toEqual(['real'])
  })

  test('an empty or broken link is not one', () => {
    expect(linksOf('[[ ]] [[|x]] [[open\nclose]] [single]')).toEqual([])
  })

  test('the line a note is mentioned on', () => {
    expect(mentionOf('First.\n  Part of the [[Garden Plan|plan]].  \nLast.', 'garden plan')).toBe('Part of the plan.')
    expect(mentionOf('Nothing here.', 'garden plan')).toBe('')
  })

  test('are rewritten to a new title, keeping the shown text and the heading', () => {
    expect(retarget('[[Herbs]], [[herbs|the bed]], [[HERBS#Basil]], [[Tomatoes]]', 'herbs', 'Herb bed')).toBe(
      '[[Herb bed]], [[Herb bed|the bed]], [[Herb bed#Basil]], [[Tomatoes]]'
    )
  })
})

describe('tags', () => {
  test('are the words after a #, in lower case, each once, in order of the alphabet', () => {
    expect(tagsOf('#Garden notes #plans/2026 and #garden again')).toEqual(['garden', 'plans/2026'])
  })

  test('a heading, a number, a # inside a word and a link to a heading are not tags', () => {
    expect(tagsOf('# Heading\nissue #12, c#sharp, [[Note#Part]] `#code`')).toEqual([])
  })
})

describe('the line that stands for a note', () => {
  test('is its first line that is not a heading, without its marks', () => {
    expect(excerptOf('\n\n## Garden plan\n- The **beds**, see [[Herbs|the herb bed]] and [[Tomatoes]].')).toBe('The beds, see the herb bed and Tomatoes.')
    expect(excerptOf('- [ ] Milk')).toBe('[ ] Milk')
    expect(excerptOf('')).toBe('')
  })

  test('is its first heading when it says nothing else', () => {
    expect(excerptOf('# Garden plan\n\n## Beds\n')).toBe('Garden plan')
  })

  test('is cut when it is long', () => {
    expect([...excerptOf('é'.repeat(300))]).toHaveLength(161)
  })

  test('the line some words are on, whatever their capitals', () => {
    expect(lineWith('One.\nThe North bed.\nThree.', 'north')).toBe('The North bed.')
    expect(lineWith('One.', 'north')).toBe('')
  })
})

describe('a task', () => {
  const body = 'Plan\n- [ ] Seeds\n  1. [x] Frame\n- Not a task'

  test('is ticked and unticked on its line', () => {
    expect(toggleTask(body, 1)).toBe('Plan\n- [x] Seeds\n  1. [x] Frame\n- Not a task')
    expect(toggleTask(body, 2)).toBe('Plan\n- [ ] Seeds\n  1. [ ] Frame\n- Not a task')
  })

  test('a line that is not a task changes nothing', () => {
    expect(toggleTask(body, 0)).toBe(body)
    expect(toggleTask(body, 3)).toBe(body)
    expect(toggleTask(body, 9)).toBe(body)
  })
})
