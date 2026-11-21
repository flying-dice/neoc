import { describe, expect, test } from 'bun:test'
import {
  asPath,
  isRoute,
  MAX_DEPTH,
  NavRegistry,
  popRoute,
  popToRoot,
  pushRoute,
  reconcile,
  removeEntry,
  type StackEntry,
  sameRoute,
  type NavHandle
} from './nav'

const handle = (key: string, path: string, log: string[] = []): NavHandle => ({
  key,
  path,
  push: (r) => void log.push(`${key}:push:${r.name}`),
  pop: () => void log.push(`${key}:pop`),
  popToRoot: () => void log.push(`${key}:root`)
})

describe('stack operations', () => {
  test('push appends, pop removes the top, popToRoot clears', () => {
    const a = { name: 'note', id: 1 }
    const b = { name: 'note', id: 2 }
    const path = pushRoute(pushRoute([], a), b)
    expect(path).toEqual([a, b])
    expect(popRoute(path)).toEqual([a])
    expect(popRoute([])).toEqual([])
    expect(popToRoot()).toEqual([])
  })

  test('operations do not mutate the path they were given', () => {
    const path = [{ name: 'a' }]
    pushRoute(path, { name: 'b' })
    popRoute(path)
    expect(path).toEqual([{ name: 'a' }])
  })

  test('the stack has a depth limit', () => {
    let path: { name: string }[] = []
    for (let i = 0; i < MAX_DEPTH + 5; i++) path = pushRoute(path, { name: `r${i}` })
    expect(path).toHaveLength(MAX_DEPTH)
  })

  test('routes are objects with a name; the rest is params', () => {
    expect(isRoute({ name: 'note', id: 3 })).toBe(true)
    expect(isRoute({ name: '' })).toBe(false)
    expect(isRoute('note')).toBe(false)
    expect(isRoute(null)).toBe(false)
    expect(isRoute([{ name: 'x' }])).toBe(false)
  })

  test('whatever a binding holds becomes a clean path', () => {
    expect(asPath(null)).toEqual([])
    expect(asPath('x')).toEqual([])
    expect(asPath([{ name: 'a' }, 7, { nope: 1 }, { name: 'b', id: 2 }])).toEqual([
      { name: 'a' },
      { name: 'b', id: 2 }
    ])
  })

  test('same route means same name and params', () => {
    expect(sameRoute({ name: 'n', id: 1 }, { name: 'n', id: 1 })).toBe(true)
    expect(sameRoute({ name: 'n', id: 1 }, { name: 'n', id: 2 })).toBe(false)
  })
})

describe('NavRegistry', () => {
  test('an action goes to the innermost stack that contains its node', () => {
    const log: string[] = []
    const reg = new NavRegistry()
    reg.register(handle('outer', 'main/outer', log))
    reg.register(handle('inner', 'main/outer/@root/inner', log))
    reg.resolve('main/outer/@root/inner/@dest:note/screen/open')?.push({ name: 'x' })
    reg.resolve('main/outer/@root/screen/open')?.pop()
    expect(log).toEqual(['inner:push:x', 'outer:pop'])
  })

  test('a path that only shares a prefix of the name is not inside the stack', () => {
    const reg = new NavRegistry()
    reg.register(handle('nav', 'main/nav'))
    reg.register(handle('other', 'main/other'))
    expect(reg.resolve('main/navbar/button')).toBeUndefined()
  })

  test('a named stack wins, wherever the action comes from', () => {
    const log: string[] = []
    const reg = new NavRegistry()
    reg.register(handle('a', 'main/a', log))
    reg.register(handle('b', 'main/b', log))
    reg.resolve('main/a/x', 'b')?.popToRoot()
    expect(log).toEqual(['b:root'])
    expect(reg.resolve('main/a/x', 'missing')).toBeUndefined()
  })

  test('with one stack in the window, an action from outside it still finds it', () => {
    const reg = new NavRegistry()
    reg.register(handle('only', 'main/only'))
    expect(reg.resolve('main/toolbar/back')?.key).toBe('only')
  })

  test('with several stacks and no way to tell, nothing is chosen', () => {
    const reg = new NavRegistry()
    reg.register(handle('a', 'main/a'))
    reg.register(handle('b', 'main/b'))
    expect(reg.resolve('main/toolbar/back')).toBeUndefined()
  })

  test('unregistering removes the stack', () => {
    const reg = new NavRegistry()
    const off = reg.register(handle('a', 'main/a'))
    expect(reg.size).toBe(1)
    off()
    expect(reg.size).toBe(0)
    expect(reg.resolve('main/a/x')).toBeUndefined()
  })
})

describe('reconcile: screens follow the path', () => {
  let n = 0
  const id = (): string => `s${++n}`
  const live = (entries: StackEntry[]): string[] =>
    entries.filter((e) => !e.leaving).map((e) => e.route?.name ?? 'root')

  test('the first path builds a root and one screen per route', () => {
    const e = reconcile([], [{ name: 'a' }, { name: 'b' }], id)
    expect(live(e)).toEqual(['root', 'a', 'b'])
    expect(e[0].route).toBeNull()
  })

  test('a push keeps every existing screen and its id', () => {
    const first = reconcile([], [{ name: 'a' }], id)
    const next = reconcile(first, [{ name: 'a' }, { name: 'b' }], id)
    expect(next.slice(0, 2)).toEqual(first)
    expect(live(next)).toEqual(['root', 'a', 'b'])
  })

  test('a pop marks the top screen leaving; it is removed when its slide ends', () => {
    const first = reconcile([], [{ name: 'a' }, { name: 'b' }], id)
    const popped = reconcile(first, [{ name: 'a' }], id)
    expect(popped.find((e) => e.route?.name === 'b')?.leaving).toBe(true)
    expect(live(popped)).toEqual(['root', 'a'])
    const done = removeEntry(popped, first[2].id)
    expect(done.map((e) => e.id)).toEqual([first[0].id, first[1].id])
  })

  test('popToRoot drops everything above the root; pushing a different route replaces the top', () => {
    const first = reconcile([], [{ name: 'a', id: 1 }, { name: 'b' }], id)
    expect(live(reconcile(first, [], id))).toEqual(['root'])
    const swapped = reconcile(first, [{ name: 'a', id: 1 }, { name: 'c' }], id)
    expect(live(swapped)).toEqual(['root', 'a', 'c'])
    expect(swapped.filter((e) => e.leaving).map((e) => e.route?.name)).toEqual(['b'])
  })

  test('a route with other params is another screen', () => {
    const first = reconcile([], [{ name: 'note', id: 1 }], id)
    const next = reconcile(first, [{ name: 'note', id: 2 }], id)
    expect(next.find((e) => !e.leaving && e.route)?.id).not.toBe(first[1].id)
  })

  test('a screen that is still leaving stays through the next change', () => {
    const a = reconcile([], [{ name: 'a' }, { name: 'b' }], id)
    const b = reconcile(a, [{ name: 'a' }], id)
    const c = reconcile(b, [{ name: 'a' }, { name: 'x' }], id)
    expect(c.filter((e) => e.leaving)).toHaveLength(1)
    expect(live(c)).toEqual(['root', 'a', 'x'])
  })
})
