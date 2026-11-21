import { describe, expect, test } from 'bun:test'
import { imageSource } from './image'

describe('imageSource', () => {
  test('urls and bare strings pass through to the kit, which decides; anything else is nothing', () => {
    expect(imageSource({ url: 'https://example.org/a.png' })).toBe('https://example.org/a.png')
    expect(imageSource('data:image/png;base64,AA')).toBe('data:image/png;base64,AA')
    expect(imageSource({ asset: 'logo.png' })).toBeUndefined()
    expect(imageSource({ file: 'a.png' })).toBeUndefined()
    expect(imageSource({ url: 42 })).toBeUndefined()
    expect(imageSource(null)).toBeUndefined()
    expect(imageSource([])).toBeUndefined()
  })
})
