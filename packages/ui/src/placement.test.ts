import { describe, expect, test } from 'bun:test'
import { MENU_GAP, placeMenu } from './placement'

const screen = { width: 800, height: 600 }
const menu = { width: 200, height: 100 }

describe('placeMenu', () => {
  test('below the anchor, centred on it', () => {
    expect(placeMenu({ left: 300, top: 100, width: 60, height: 60 }, menu, screen)).toEqual({
      left: 230,
      top: 160 + MENU_GAP,
      side: 'below'
    })
  })

  test('above when there is no room below (an icon of the Dock)', () => {
    expect(placeMenu({ left: 300, top: 520, width: 60, height: 60 }, menu, screen)).toEqual({
      left: 230,
      top: 520 - MENU_GAP - 100,
      side: 'above'
    })
  })

  test('kept inside the left and right edges', () => {
    expect(placeMenu({ left: 0, top: 100, width: 60, height: 60 }, menu, screen).left).toBe(MENU_GAP)
    expect(placeMenu({ left: 760, top: 100, width: 40, height: 60 }, menu, screen).left).toBe(
      800 - 200 - MENU_GAP
    )
  })

  test('with room on neither side it takes the roomier one and stays on screen', () => {
    const placed = placeMenu({ left: 300, top: 40, width: 60, height: 60 }, menu, { width: 800, height: 180 })
    expect(placed.side).toBe('below')
    expect(placed.top).toBe(180 - 100 - MENU_GAP)
  })

  test('a menu bigger than the screen starts at the gap', () => {
    expect(placeMenu({ left: 10, top: 10, width: 10, height: 10 }, { width: 900, height: 700 }, screen)).toMatchObject({
      left: MENU_GAP,
      top: MENU_GAP
    })
  })
})
