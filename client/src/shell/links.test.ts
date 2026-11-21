/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { openExternal } from './links'

function fakeWindow() {
  const calls: string[][] = []
  const assigned: string[] = []
  return {
    calls,
    assigned,
    target: {
      open: (...args: string[]) => void calls.push(args),
      location: { assign: (url: string) => void assigned.push(url) }
    }
  }
}

describe('openExternal', () => {
  test('a web page opens in a new tab with no opener and no referrer', () => {
    const w = fakeWindow()
    expect(openExternal('https://example.org/a', w.target)).toBe(true)
    expect(w.calls).toEqual([['https://example.org/a', '_blank', 'noopener,noreferrer']])
  })

  test('mailto hands over to the mail client without opening a tab', () => {
    const w = fakeWindow()
    expect(openExternal('mailto:a@example.org', w.target)).toBe(true)
    expect(w.assigned).toEqual(['mailto:a@example.org'])
    expect(w.calls).toEqual([])
  })

  test('anything else is refused and nothing is opened', () => {
    const w = fakeWindow()
    expect(openExternal('javascript:alert(1)', w.target)).toBe(false)
    expect(w.calls).toEqual([])
    expect(w.assigned).toEqual([])
  })
})
