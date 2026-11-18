import { describe, expect, test } from 'bun:test'
import type { Component } from 'svelte'
import { ComponentRegistry, createRegistry, needsDecorator, type AdapterProps } from './registry'

/** A stand-in for a Svelte 5 component (a function): the registry only stores it. */
const fake = (name: string): Component<AdapterProps> =>
  Object.assign(() => undefined, { displayName: name }) as unknown as Component<AdapterProps>

describe('the component registry', () => {
  test('register takes a spec or a bare component, and replaces by type', () => {
    const r = createRegistry()
    const stack = fake('Stack')
    r.register('Stack', stack).register('Gauge', { component: fake('Gauge'), since: '1.2' })
    expect(r.get('Stack')?.component).toBe(stack)
    expect(r.get('Gauge')).toMatchObject({ since: '1.2' })
    const next = fake('Stack2')
    r.register('Stack', next)
    expect(r.get('Stack')?.component).toBe(next)
  })

  test('an unknown type has no spec: the renderer shows the fallback or Unsupported', () => {
    const r = new ComponentRegistry()
    expect(r.get('Nope')).toBeUndefined()
    expect(r.has('Nope')).toBe(false)
  })

  test('types are sorted and unregister removes', () => {
    const r = createRegistry().register('b', fake('b')).register('a', fake('a'))
    expect(r.types()).toEqual(['a', 'b'])
    expect(r.unregister('a')).toBe(true)
    expect(r.unregister('a')).toBe(false)
    expect(r.types()).toEqual(['b'])
  })

  test('bindings are declared per component, for the binding-writability check', () => {
    const r = createRegistry().register('TextField', {
      component: fake('TextField'),
      bindings: ['text']
    })
    expect(r.get('TextField')?.bindings).toEqual(['text'])
  })
})

describe('the decorator', () => {
  test('only nodes with a contextMenu slot, drag or drop need it', () => {
    expect(needsDecorator({ type: 'Text', key: 'a' })).toBe(false)
    expect(needsDecorator({ type: 'Text', key: 'a', slots: { contextMenu: [] } })).toBe(false)
    expect(
      needsDecorator({
        type: 'Text',
        key: 'a',
        slots: { contextMenu: [{ type: 'Button', key: 'b' }] }
      })
    ).toBe(true)
    expect(needsDecorator({ type: 'Text', key: 'a', props: { drag: { type: 'task' } } })).toBe(true)
    expect(
      needsDecorator({ type: 'Stack', key: 'a', props: { drop: { accepts: ['task'] } } })
    ).toBe(true)
  })

  test('a registry has none until the kit sets one', () => {
    expect(createRegistry().decorator).toBeNull()
  })
})
