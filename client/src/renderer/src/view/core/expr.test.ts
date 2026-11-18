import { describe, expect, test } from 'bun:test'
import type { Expr } from '../protocol'
import { evaluate, FUNCTION_NAMES, MAX_EXPR_NODES, type FormatEnv } from './expr'

const env: FormatEnv = { locale: 'en-GB', timeZone: 'Europe/London', now: Date.UTC(2026, 10, 18, 12, 0, 0) }
const scope: Record<string, unknown> = { 'client.search': 'gro', 'src.s.total': 3, 'row.title': 'Plan', 'state.n': 0, 'row.tags': ['a', 'b', 'c'] }
const run = (e: Expr): unknown => evaluate(e, (r) => scope[r], env)
const fn = (name: string, ...args: Expr[]): Expr => ({ $fn: name, args })

describe('literals and refs', () => {
  test('literals pass through, refs resolve, arrays and objects are walked', () => {
    expect(run('x')).toBe('x')
    expect(run(7)).toBe(7)
    expect(run(null)).toBeNull()
    expect(run({ $ref: 'client.search' })).toBe('gro')
    expect(run([{ $ref: 'row.title' }, 1])).toEqual(['Plan', 1])
    expect(run({ a: { $ref: 'row.title' }, b: 2 })).toEqual({ a: 'Plan', b: 2 })
  })
  test('an unknown ref is undefined; $lit is never interpreted', () => {
    expect(run({ $ref: 'client.nope' })).toBeUndefined()
    expect(run({ $lit: { $ref: 'client.search' } })).toEqual({ $ref: 'client.search' })
  })
  test('undefined expression is undefined', () => {
    expect(evaluate(undefined, () => 1, env)).toBeUndefined()
  })
})

describe('the closed function set', () => {
  test('logic and comparison', () => {
    expect(run(fn('not', true))).toBe(false)
    expect(run(fn('and', true, 1, 'x'))).toBe(true)
    expect(run(fn('and', true, 0))).toBe(false)
    expect(run(fn('or', 0, '', 'x'))).toBe(true)
    expect(run(fn('eq', { $ref: 'client.search' }, 'gro'))).toBe(true)
    expect(run(fn('ne', 1, 2))).toBe(true)
    expect(run(fn('lt', 1, 2))).toBe(true)
    expect(run(fn('gt', 1, 2))).toBe(false)
  })
  test('arithmetic, if and coalesce', () => {
    expect(run(fn('add', 2, 3))).toBe(5)
    expect(run(fn('sub', 2, 3))).toBe(-1)
    expect(run(fn('if', true, 'a', 'b'))).toBe('a')
    expect(run(fn('coalesce', null, '', 'x', 'y'))).toBe('x')
    expect(run(fn('coalesce', null, ''))).toBeUndefined()
  })
  test('strings and collections', () => {
    expect(run(fn('concat', { $ref: 'src.s.total' }, ' notes'))).toBe('3 notes')
    expect(run(fn('concat', null, 'a', { $ref: 'client.nope' }))).toBe('a')
    expect(run(fn('len', { $ref: 'row.tags' }))).toBe(3)
    expect(run(fn('len', 'abcd'))).toBe(4)
    expect(run(fn('isEmpty', ''))).toBe(true)
    expect(run(fn('isEmpty', []))).toBe(true)
    expect(run(fn('isEmpty', 0))).toBe(false)
    expect(run(fn('upper', 'ab'))).toBe('AB')
    expect(run(fn('lower', 'AB'))).toBe('ab')
  })
  test('if is lazy: the untaken branch is never resolved', () => {
    const seen: string[] = []
    evaluate(fn('if', true, { $ref: 'a' }, { $ref: 'b' }), (r) => (seen.push(r), 1), env)
    expect(seen).toEqual(['a'])
  })
  test('an unknown function is undefined, never a throw', () => {
    expect(run(fn('eval', 'process.exit()'))).toBeUndefined()
    expect(FUNCTION_NAMES).not.toContain('eval')
  })
  test('the set is exactly the spec set', () => {
    expect([...FUNCTION_NAMES].sort()).toEqual(
      [
        ...'not and or eq ne lt gt add sub if coalesce concat len isEmpty upper lower'.split(' '),
        ...'fmt.number fmt.currency fmt.percent fmt.bytes fmt.date fmt.time fmt.dateTime fmt.relative fmt.plural fmt.list'.split(' ')
      ].sort()
    )
  })
})

describe('fmt.* uses the viewer locale and zone', () => {
  test('numbers, percent, bytes', () => {
    expect(run(fn('fmt.number', 1234.5))).toBe('1,234.5')
    expect(run(fn('fmt.percent', 0.256))).toBe('26%')
    expect(run(fn('fmt.bytes', 1_500_000))).toBe('1.5 MB')
    expect(run(fn('fmt.bytes', 12))).toBe('12 byte')
  })
  test('currency from minor units and from a bare number', () => {
    expect(run(fn('fmt.currency', { $lit: { amount: 1999, currency: 'GBP' } }))).toBe('£19.99')
    expect(run(fn('fmt.currency', 5, 'USD'))).toBe('US$5.00')
    expect(run(fn('fmt.currency', 5, 'not-a-currency'))).toBeUndefined()
  })
  test('an instant is formatted in the viewer zone; a floating date is not shifted', () => {
    const london = evaluate(fn('fmt.time', '2026-07-01T23:30:00Z'), () => undefined, env)
    expect(london).toBe('00:30') // BST: the same instant, the viewer's wall clock
    const date = evaluate(fn('fmt.date', '2026-11-18', 'short'), () => undefined, { ...env, timeZone: 'Pacific/Auckland' })
    expect(date).toBe('18/11/2026') // a due date stays the 18th everywhere
  })
  test('relative times tick from env.now', () => {
    expect(run(fn('fmt.relative', '2026-11-18T11:00:00Z'))).toBe('1 hour ago')
    expect(run(fn('fmt.relative', '2026-11-17T12:00:00Z'))).toBe('yesterday')
    expect(run(fn('fmt.relative', '2026-11-18T12:00:00Z'))).toBe('this minute')
  })
  test('plural and list', () => {
    expect(run(fn('fmt.plural', 1, { $lit: { one: '{n} note', other: '{n} notes' } }))).toBe('1 note')
    expect(run(fn('fmt.plural', 1200, { $lit: { one: '{n} note', other: '{n} notes' } }))).toBe('1,200 notes')
    expect(run(fn('fmt.list', ['a', 'b', 'c']))).toBe('a, b and c')
  })
})

describe('the cost bound', () => {
  test('an expression over the node budget is undefined instead of hanging', () => {
    let deep: Expr = 1
    for (let i = 0; i < MAX_EXPR_NODES + 5; i++) deep = fn('add', deep, 1)
    expect(run(deep)).toBeUndefined()
  })
  test('one just inside the budget still evaluates', () => {
    let e: Expr = 0
    for (let i = 0; i < 300; i++) e = fn('add', e, 1)
    expect(run(e)).toBe(300)
  })
})
