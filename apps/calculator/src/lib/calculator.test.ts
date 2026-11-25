import { describe, expect, test } from 'bun:test'
import type { PressParams, Screen } from '@calculator/rpc'
import { Calculator, type CalculatorRpc, type KeyStroke } from './calculator.svelte'
import { createFailingApi, createFakeApi } from './fakeApi'

const stroke = (key: string, modifiers: Partial<KeyStroke> = {}): KeyStroke => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...modifiers
})

/** A backend that records what it was sent and answers only when the test says so. */
function manualApi(inner: CalculatorRpc = createFakeApi()) {
  const sent: PressParams[] = []
  const waiting: (() => void)[] = []
  const rpc: CalculatorRpc = {
    press: (params) => {
      sent.push(params)
      return new Promise<Screen>((resolve, reject) => {
        waiting.push(() => inner.press(params).then(resolve, reject))
      })
    }
  }
  /** Everything already under way has happened (a key is sent on a later turn than the one it was pressed in). */
  const settle = (): Promise<void> => Bun.sleep(0)
  /** Let the oldest unanswered key be answered, and wait for the answer to land. */
  const answer = async (): Promise<void> => {
    await settle()
    waiting.shift()?.()
    await settle()
  }
  return { rpc, sent, answer, settle }
}

describe('a calculator that has just opened', () => {
  test('shows 0, no caption, AC, and nothing to announce', () => {
    const calculator = new Calculator(createFakeApi())
    expect(calculator.display).toBe('0')
    expect(calculator.caption).toBe('')
    expect(calculator.clearLabel).toBe('AC')
    expect(calculator.pending).toBeUndefined()
    expect(calculator.announcement).toBe('')
  })

  test('sends its first key with no state', async () => {
    const { rpc, sent, answer } = manualApi()
    const calculator = new Calculator(rpc)
    void calculator.press('7')
    await answer()
    expect(sent).toEqual([{ key: '7', state: undefined }])
    expect(calculator.display).toBe('7')
    expect(calculator.clearLabel).toBe('C')
  })
})

describe('pressing keys', () => {
  test('a sum: the waiting operator, then the result and what it was', async () => {
    const calculator = new Calculator(createFakeApi())
    void calculator.press('1')
    void calculator.press('2')
    await calculator.press('+')
    expect(calculator.pending).toBe('+')
    expect(calculator.caption).toBe('12 +')
    void calculator.press('7')
    await calculator.press('=')
    expect(calculator.display).toBe('19')
    expect(calculator.caption).toBe('12 + 7')
    expect(calculator.pending).toBeUndefined()
  })

  test('keys go one at a time, in order, each with the state the one before answered with', async () => {
    const { rpc, sent, answer, settle } = manualApi()
    const calculator = new Calculator(rpc)
    void calculator.press('1')
    void calculator.press('2')
    const last = calculator.press('3')
    await settle()
    // nothing is answered yet: only the first key has gone
    expect(sent).toHaveLength(1)
    await answer()
    expect(sent).toHaveLength(2)
    expect(sent[1]?.state?.entry).toBe('1')
    await answer()
    expect(sent[2]?.state?.entry).toBe('12')
    await answer()
    await last
    expect(calculator.display).toBe('123')
  })

  test('a sum that fails is on the screen: Error, the reason, and the reason announced', async () => {
    const calculator = new Calculator(createFakeApi())
    void calculator.press('8')
    void calculator.press('/')
    void calculator.press('0')
    await calculator.press('=')
    expect(calculator.display).toBe('Error')
    expect(calculator.caption).toBe("Can't divide by zero")
    expect(calculator.announcement).toBe("Can't divide by zero")
    await calculator.press('clear')
    expect(calculator.display).toBe('0')
    expect(calculator.announcement).toBe('')
  })
})

describe('a key that does not reach the backend', () => {
  test('says why in the caption, keeps the display, and announces nothing (it is not the sum)', async () => {
    const calculator = new Calculator(createFailingApi('The host is not answering'))
    await calculator.press('5')
    expect(calculator.caption).toBe('The host is not answering')
    expect(calculator.display).toBe('0')
    expect(calculator.announcement).toBe('')
  })

  test('a rejection that is not an Error is shown as text', async () => {
    const calculator = new Calculator({ press: () => Promise.reject('offline') })
    await calculator.press('5')
    expect(calculator.caption).toBe('offline')
  })

  test('the next key that is answered clears the failure and carries on from the last good state', async () => {
    const fake = createFakeApi()
    let down = false
    const { rpc, sent, answer } = manualApi({
      press: (params) => (down ? Promise.reject(new Error('Connection lost')) : fake.press(params))
    })
    const calculator = new Calculator(rpc)
    void calculator.press('4')
    await answer()
    down = true
    void calculator.press('2')
    await answer()
    expect(calculator.caption).toBe('Connection lost')
    expect(calculator.display).toBe('4')
    down = false
    void calculator.press('2')
    await answer()
    expect(sent[2]?.state?.entry).toBe('4')
    expect(calculator.display).toBe('42')
    expect(calculator.caption).toBe('')
  })
})

describe('the keyboard', () => {
  test('a typed key is pressed and taken', async () => {
    const { rpc, sent, settle } = manualApi()
    const calculator = new Calculator(rpc)
    expect(calculator.keyDown(stroke('6'))).toBe(true)
    await settle()
    expect(sent.map((p) => p.key)).toEqual(['6'])
  })

  test('a key that is not the keypad’s is left alone', async () => {
    const { rpc, sent, settle } = manualApi()
    const calculator = new Calculator(rpc)
    expect(calculator.keyDown(stroke('a'))).toBe(false)
    await settle()
    expect(sent).toEqual([])
  })

  test.each(['metaKey', 'ctrlKey', 'altKey'] as const)('a shortcut (%s) is left alone', async (modifier) => {
    const { rpc, sent, settle } = manualApi()
    const calculator = new Calculator(rpc)
    expect(calculator.keyDown(stroke('1', { [modifier]: true }))).toBe(false)
    await settle()
    expect(sent).toEqual([])
  })

  test('Enter is =', async () => {
    const { rpc, sent, settle } = manualApi()
    const calculator = new Calculator(rpc)
    expect(calculator.keyDown(stroke('Enter'))).toBe(true)
    await settle()
    expect(sent.map((p) => p.key)).toEqual(['='])
  })

  test('after Tab, Enter is the focused key’s own; after typing or a pointer, it is = again', async () => {
    const { rpc, sent, answer } = manualApi()
    const calculator = new Calculator(rpc)
    expect(calculator.keyDown(stroke('Tab'))).toBe(false)
    expect(calculator.keyDown(stroke('Enter'))).toBe(false)
    expect(calculator.keyDown(stroke('2'))).toBe(true)
    expect(calculator.keyDown(stroke('Enter'))).toBe(true)
    calculator.keyDown(stroke('Tab'))
    calculator.pointerDown()
    expect(calculator.keyDown(stroke('Enter'))).toBe(true)
    for (let i = 0; i < 3; i++) await answer()
    expect(sent.map((p) => p.key)).toEqual(['2', '=', '='])
  })
})
