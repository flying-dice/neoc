import { describe, expect, test } from 'bun:test'
import { fold, MATCH, matchRank } from './spotlightMatch'

describe('fold', () => {
  test('case, accents and outer spaces do not count', () => {
    expect(fold('  Café Olé ')).toBe('cafe ole')
    expect(fold('ÅNGSTRÖM')).toBe('angstrom')
    expect(fold('Notes')).toBe('notes')
  })
})

describe('matchRank', () => {
  test('the start of the text is the best match', () => {
    expect(matchRank('calc', 'Calculator')).toBe(MATCH.prefix)
    expect(matchRank('Calculator', 'Calculator')).toBe(MATCH.prefix)
    expect(matchRank('show home', 'Show Home Screen')).toBe(MATCH.prefix)
  })

  test('the start of a later word is next', () => {
    expect(matchRank('home', 'Show Home Screen')).toBe(MATCH.wordStart)
    expect(matchRank('svelte', 'Tasks (Svelte)')).toBe(MATCH.wordStart)
    expect(matchRank('up', 'Follow-up')).toBe(MATCH.wordStart)
  })

  test('every word typed must start a word, in any order', () => {
    expect(matchRank('home scr', 'Show Home Screen')).toBe(MATCH.wordStart)
    expect(matchRank('screen show', 'Show Home Screen')).toBe(MATCH.wordStart)
    expect(matchRank('home dock', 'Show Home Screen')).toBeNull()
  })

  test('somewhere inside a word is the weakest match', () => {
    expect(matchRank('ulat', 'Calculator')).toBe(MATCH.substring)
    expect(matchRank('ome ree', 'Show Home Screen')).toBe(MATCH.substring)
  })

  test('prefix before word start before substring', () => {
    const ranks = ['Notes', 'Quick Notes', 'Keynotes'].map((name) => matchRank('note', name))
    expect(ranks).toEqual([MATCH.prefix, MATCH.wordStart, MATCH.substring])
    expect(MATCH.prefix).toBeLessThan(MATCH.wordStart)
    expect(MATCH.wordStart).toBeLessThan(MATCH.substring)
  })

  test('case does not matter, either way', () => {
    expect(matchRank('CALC', 'calculator')).toBe(MATCH.prefix)
    expect(matchRank('calc', 'CALCULATOR')).toBe(MATCH.prefix)
  })

  test('accents do not matter, either way', () => {
    expect(matchRank('cafe', 'Café Menu')).toBe(MATCH.prefix)
    expect(matchRank('café', 'Cafe Menu')).toBe(MATCH.prefix)
    expect(matchRank('uber', 'Über uns')).toBe(MATCH.prefix)
    expect(matchRank('menu', 'Café Menü')).toBe(MATCH.wordStart)
  })

  test('no match, an empty query and an empty text', () => {
    expect(matchRank('xyz', 'Calculator')).toBeNull()
    expect(matchRank('', 'Calculator')).toBeNull()
    expect(matchRank('   ', 'Calculator')).toBeNull()
    expect(matchRank('a', '')).toBeNull()
  })

  test('spaces around what was typed are ignored', () => {
    expect(matchRank('  calc ', 'Calculator')).toBe(MATCH.prefix)
  })
})
