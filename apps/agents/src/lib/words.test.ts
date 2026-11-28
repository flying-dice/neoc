import { describe, expect, test } from 'bun:test'
import { textOf, wordsOf } from './words'

describe('a command typed on one line', () => {
  test('is its words between spaces', () => {
    expect(wordsOf('claude-code-acp')).toEqual(['claude-code-acp'])
    expect(wordsOf('  npx  -y   @zed-industries/claude-code-acp ')).toEqual(['npx', '-y', '@zed-industries/claude-code-acp'])
    expect(wordsOf('')).toEqual([])
    expect(wordsOf('   ')).toEqual([])
  })

  test('keeps a word with a space in it in quotes', () => {
    expect(wordsOf('sh -c "echo hi there"')).toEqual(['sh', '-c', 'echo hi there'])
    expect(wordsOf("sh -c 'echo \"hi\"'")).toEqual(['sh', '-c', 'echo "hi"'])
    expect(wordsOf('say "a \\"quote\\" and a \\\\"')).toEqual(['say', 'a "quote" and a \\'])
    expect(wordsOf('a "" b')).toEqual(['a', '', 'b'])
    expect(wordsOf('--name="Ada Lovelace"')).toEqual(['--name=Ada Lovelace'])
  })

  test('runs an open quote to the end', () => {
    expect(wordsOf('echo "open')).toEqual(['echo', 'open'])
  })
})

describe('words written on one line', () => {
  test('are joined by spaces, a word that needs it quoted', () => {
    expect(textOf(['claude-code-acp', '--verbose'])).toBe('claude-code-acp --verbose')
    expect(textOf(['sh', '-c', 'echo hi'])).toBe('sh -c "echo hi"')
    expect(textOf(['a', ''])).toBe('a ""')
    expect(textOf(['say "hi"', 'C:\\x'])).toBe('"say \\"hi\\"" "C:\\\\x"')
  })

  test('read back as the same words', () => {
    for (const words of [['npx', '-y', 'pkg'], ['sh', '-c', 'echo "a b" \\ c'], ['', ' ', "it's"], []]) expect(wordsOf(textOf(words))).toEqual(words)
  })
})
