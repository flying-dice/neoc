import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * `crates/vos-http/src/apps/bridge.js` run against a stand-in `window`: what an app's call posts to the host page. It is the
 * script every app's page loads, so this is the one place a framework's objects meet `postMessage`.
 */
const source = readFileSync(join(import.meta.dir, '../crates/vos-http/src/apps/bridge.js'), 'utf8')

function load(): { posted: Record<string, unknown>[]; vos: { call(method: string, params?: unknown): Promise<unknown> } } {
  const posted: Record<string, unknown>[] = []
  const html = { dataset: {} as Record<string, string>, style: {} as Record<string, string> }
  const listeners: Record<string, ((e: unknown) => void)[]> = {}
  const host = {
    // like the real thing, a message that cannot be structured-cloned throws
    postMessage: (message: Record<string, unknown>) => {
      structuredClone(message)
      posted.push(message)
    }
  }
  const win: Record<string, unknown> = {
    parent: host,
    addEventListener: (name: string, fn: (e: unknown) => void) => (listeners[name] ??= []).push(fn),
    matchMedia: () => ({ matches: false }),
    location: { search: '' }
  }
  const doc = {
    documentElement: html,
    readyState: 'loading',
    addEventListener: () => undefined,
    getElementById: () => null,
    body: null
  }
  new Function('window', 'parent', 'document', 'location', 'URLSearchParams', source).call(
    win,
    win,
    host,
    doc,
    win.location,
    URLSearchParams
  )
  return { posted, vos: win.vos as never }
}

describe('the app side of the bridge posts plain JSON', () => {
  test('a reactive proxy as the arguments is posted as the plain object it stands for', async () => {
    const { posted, vos } = load()
    // what a Svelte 5 `$state` object is to `postMessage`: a Proxy, which structuredClone refuses
    const reactive = new Proxy({ title: 'Buy milk', tags: ['a'] }, {})
    expect(() => structuredClone(reactive)).toThrow()
    void vos.call('add', reactive)
    const call = posted.find((m) => m.method === 'add')
    expect(call?.params).toEqual({ title: 'Buy milk', tags: ['a'] })
  })

  test('no arguments are an empty object, and arguments that are not JSON reject with the method named', async () => {
    const { posted, vos } = load()
    void vos.call('list')
    expect(posted.find((m) => m.method === 'list')?.params).toEqual({})
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    await expect(vos.call('add', cyclic)).rejects.toThrow(/The arguments of "add" are not JSON/)
    await expect(vos.call('add', () => 1)).rejects.toThrow(/The arguments of "add" are not JSON: it is not JSON/)
    expect(posted.filter((m) => m.method === 'add')).toEqual([])
  })
})
