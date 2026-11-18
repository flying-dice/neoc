import { describe, expect, test } from 'bun:test'
import { imageSource } from './image'

describe('imageSource', () => {
  test('a bundled asset maps outside the client build’s /assets/ (hashed files, cached for good by the service worker)', () => {
    expect(imageSource({ asset: 'logo.png' })).toBe('/app-assets/logo.png')
    expect(imageSource({ asset: 'logo.png' })?.startsWith('/assets/')).toBe(false)
  })

  test('only plain file names are assets', () => {
    for (const asset of ['../x', 'a/b', '.hidden', '', 'x y', 'x?y'])
      expect(imageSource({ asset })).toBeUndefined()
  })

  test('urls and bare strings pass through to the kit, which decides; anything else is nothing', () => {
    expect(imageSource({ url: 'https://example.org/a.png' })).toBe('https://example.org/a.png')
    expect(imageSource('data:image/png;base64,AA')).toBe('data:image/png;base64,AA')
    expect(imageSource({ file: 'a.png' })).toBeUndefined()
    expect(imageSource(null)).toBeUndefined()
    expect(imageSource([])).toBeUndefined()
  })
})
