import { describe, expect, test } from 'bun:test'
import { focusFrame } from './frameFocus'

const body = {}
const frame = (): { frame: { contentWindow: { focus(): void } | null }; focused(): number } => {
  let focused = 0
  return { frame: { contentWindow: { focus: () => void focused++ } }, focused: () => focused }
}
const element = (launcher: boolean): Element =>
  ({
    closest: (selector: string) => (launcher && selector === '[data-launcher]' ? {} : null)
  }) as unknown as Element

describe('the frame takes the keyboard once it is ready', () => {
  test('when nothing holds it (the standalone page: the body has focus), the frame is focused', () => {
    const f = frame()
    expect(focusFrame(f.frame, { activeElement: body as never, body })).toBe(true)
    expect(f.focused()).toBe(1)
    expect(focusFrame(f.frame, { activeElement: null, body })).toBe(true)
  })
  test('after a launcher (the Dock opened the window) the frame is focused too', () => {
    const f = frame()
    expect(focusFrame(f.frame, { activeElement: element(true), body })).toBe(true)
    expect(f.focused()).toBe(1)
  })
  test('focus that is somewhere meaningful (a field, a dialog) is left where it is', () => {
    const f = frame()
    expect(focusFrame(f.frame, { activeElement: element(false), body })).toBe(false)
    expect(f.focused()).toBe(0)
  })
  test('a frame with no window yet is not an error', () => {
    expect(focusFrame({ contentWindow: null }, { activeElement: null, body })).toBe(true)
  })
})
