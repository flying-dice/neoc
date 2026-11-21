import { describe, expect, test } from 'bun:test'
import { uniqueBy } from './unique'

describe('uniqueBy', () => {
  test('keeps the first of every key, in order', () => {
    const unique = uniqueBy((x: { id: string; n: number }) => x.id)
    expect(
      unique([
        { id: 'a', n: 1 },
        { id: 'b', n: 2 },
        { id: 'a', n: 3 }
      ])
    ).toEqual([
      { id: 'a', n: 1 },
      { id: 'b', n: 2 }
    ])
  })

  test('an empty list stays empty and the input is not changed', () => {
    const input = [{ id: 'a' }, { id: 'a' }]
    expect(uniqueBy((x: { id: string }) => x.id)([])).toEqual([])
    uniqueBy((x: { id: string }) => x.id)(input)
    expect(input).toHaveLength(2)
  })
})
