/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { createDomEffects } from './effects'

const effects = (userActivation?: () => { isActive: boolean } | undefined) =>
  createDomEffects({ app: 'a', root: () => null, userActivation })

describe('user activation gate for the clipboard', () => {
  test('is closed when the browser says the person has not just tapped or pressed a key', () => {
    expect(effects(() => ({ isActive: false })).userActive?.()).toBe(false)
  })

  test('is open right after a tap or key press', () => {
    expect(effects(() => ({ isActive: true })).userActive?.()).toBe(true)
  })

  test('is open on a browser without the API (it enforces the rule inside writeText)', () => {
    expect(effects(() => undefined).userActive?.()).toBe(true)
  })
})
