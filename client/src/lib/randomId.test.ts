import { afterEach, expect, test } from 'bun:test'
import { randomId } from './randomId'

const original = crypto.randomUUID

afterEach(() => {
  Object.defineProperty(crypto, 'randomUUID', {
    value: original,
    configurable: true,
    writable: true
  })
})

test('an id is made where crypto.randomUUID does not exist (a plain http LAN address)', () => {
  Object.defineProperty(crypto, 'randomUUID', {
    value: undefined,
    configurable: true,
    writable: true
  })
  const id = randomId()
  expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
})

test('two ids differ', () => {
  expect(randomId()).not.toBe(randomId())
})
