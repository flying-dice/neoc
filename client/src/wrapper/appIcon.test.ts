import { describe, expect, test } from 'bun:test'
import { icons } from '@vos/ui'
import { appIconSvg, ICON_SIZE, lighten } from './appIcon'

describe('the icon of an app on its own page', () => {
  test('is the app’s symbol in white on a full square of its tint, lighter at the top', () => {
    const svg = appIconSvg('folder', 'green')
    expect(svg).toContain(`viewBox="0 0 ${ICON_SIZE} ${ICON_SIZE}"`)
    expect(svg).toContain(`<rect width="${ICON_SIZE}" height="${ICON_SIZE}" fill="url(#tint)"/>`)
    expect(svg).toContain(`stop-color="${lighten('#34c759', 0.78)}"`)
    expect(svg).toContain('<stop offset="1" stop-color="#34c759"/>')
    // `color` too: a symbol's filled parts are `currentColor`
    expect(svg).toContain(`stroke="#fff" color="#fff"`)
    expect(svg).toContain(icons.folder)
  })

  test('an app that declares no symbol or tint, or ones the kit does not have, gets the generic ones', () => {
    for (const svg of [
      appIconSvg(undefined, undefined),
      appIconSvg('no-such-symbol', 'no-such-tint'),
      appIconSvg('', '')
    ]) {
      expect(svg).toContain(icons['square.grid'])
      expect(svg).toContain('<stop offset="1" stop-color="#0066cc"/>')
    }
  })

  test('nothing an app declares is written into the picture', () => {
    const svg = appIconSvg('"><script>alert(1)</script>', '"><script>')
    expect(svg).not.toContain('script')
  })
})

describe('lightening a colour', () => {
  test('mixes it with white by the share given', () => {
    expect(lighten('#000000', 1)).toBe('#000000')
    expect(lighten('#000000', 0)).toBe('#ffffff')
    expect(lighten('#000000', 0.5)).toBe('#808080')
    expect(lighten('#ff0080', 0.5)).toBe('#ff80c0')
  })
})
