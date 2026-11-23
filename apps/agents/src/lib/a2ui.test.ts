import { describe, expect, test } from 'bun:test'
import { childrenOf, drawable, optionsOf, tabsOf, mediaUrl, resolve, surfacesOf, textOf, userAction, valueAt, withValueAt, type Component } from './a2ui'

const V09 = {
  a2ui_operations: [
    { version: 'v0.9', createSurface: { surfaceId: 's1', catalogId: 'basic' } },
    {
      version: 'v0.9',
      updateComponents: {
        surfaceId: 's1',
        components: [
          { id: 'root', component: 'Column', children: ['title', 'go'] },
          { id: 'title', component: 'Text', text: { path: '/user/name' } },
          { id: 'go', component: 'Button', child: 'title', action: { event: { name: 'go', context: { who: { path: '/user/name' }, n: 2 } } } }
        ]
      }
    },
    { version: 'v0.9', updateDataModel: { surfaceId: 's1', path: '/user', value: { name: 'Ada', age: 36 } } },
    { version: 'v0.9', updateDataModel: { surfaceId: 's1', path: '/user/age' } }
  ]
}

describe('the operations of an activity', () => {
  test('0.9: a surface is created, given components and data, and a value left out removes its key', () => {
    const [surface] = surfacesOf(V09)
    expect(surface.id).toBe('s1')
    expect([...surface.components.keys()]).toEqual(['root', 'title', 'go'])
    expect(surface.data).toEqual({ user: { name: 'Ada' } })
  })

  test('1.0: components and data arrive inside createSurface, and a null value removes its key', () => {
    const [surface] = surfacesOf({
      a2ui_operations: [
        {
          version: '1.0',
          createSurface: { surfaceId: 'main', components: [{ id: 'root', component: 'Text', text: { '@path': '/name' } }], dataModel: { name: 'Ada', x: 1 } }
        },
        { version: '1.0', updateDataModel: { surfaceId: 'main', path: '/x', value: null } }
      ]
    })
    expect(surface.data).toEqual({ name: 'Ada' })
    expect(textOf(surface.components.get('root')?.text, surface.data)).toBe('Ada')
  })

  test('a deleted surface is gone, and an update without a path replaces the data', () => {
    expect(surfacesOf({ a2ui_operations: [{ createSurface: { surfaceId: 'a' } }, { deleteSurface: { surfaceId: 'a' } }] })).toEqual([])
    const [surface] = surfacesOf({ a2ui_operations: [{ createSurface: { surfaceId: 'a' } }, { updateDataModel: { surfaceId: 'a', value: { k: 1 } } }] })
    expect(surface.data).toEqual({ k: 1 })
  })

  test('content that is not operations is no surface, whatever it is', () => {
    for (const content of [null, 'x', 3, {}, { a2ui_operations: 'no' }, { a2ui_operations: [null, 1, {}, { createSurface: 4 }, { updateComponents: { surfaceId: 7 } }] }])
      expect(surfacesOf(content)).toEqual([])
  })
})

describe('values', () => {
  const data = { user: { firstName: 'Ada' }, items: [{ name: 'one' }, { name: 'two' }], n: 3 }

  test('a plain value is itself, a bound one is the data at its path, in both forms', () => {
    expect(resolve('hi', data)).toBe('hi')
    expect(resolve({ path: '/n' }, data)).toBe(3)
    expect(resolve({ '@path': '/user/firstName' }, data)).toBe('Ada')
    expect(resolve({ path: '/nothing/here' }, data)).toBeUndefined()
    expect(textOf({ path: '/nothing' }, data)).toBe('')
  })

  test('a relative path starts at the item the component was repeated for', () => {
    const children = childrenOf({ path: '/items', componentId: 'row' }, data, '')
    expect(children).toEqual([
      { id: 'row', scope: '/items/0' },
      { id: 'row', scope: '/items/1' }
    ])
    expect(resolve({ path: 'name' }, data, children[1].scope)).toBe('two')
  })

  test('formatString puts data into text; another function has no value', () => {
    expect(resolve({ call: 'formatString', args: { value: 'Hello, ${/user/firstName}! \\${kept}' } }, data)).toBe('Hello, Ada! ${kept}')
    expect(resolve({ '@call': 'formatString', args: { value: '${name}' } }, data, '/items/0')).toBe('one')
    expect(resolve({ '@call': 'formatDate', args: {} }, data)).toBeUndefined()
  })

  test('a value is written without changing the data it was written into, and never to a prototype', () => {
    const next = withValueAt(data, '/user/lastName', 'Lovelace')
    expect(valueAt(next, '/user/lastName')).toBe('Lovelace')
    expect(valueAt(data, '/user/lastName')).toBeUndefined()
    expect(withValueAt({}, '/a/b', 1)).toEqual({ a: { b: 1 } })
    withValueAt({}, '/__proto__/polluted', true)
    expect(({} as { polluted?: boolean }).polluted).toBeUndefined()
  })
})

describe('what the person did', () => {
  test('a button with an event is an action with its context resolved', () => {
    const [surface] = surfacesOf(V09)
    const action = userAction(surface, surface.components.get('go') as Component, surface.data, '', new Date('2026-01-02T03:04:05Z'))
    expect(action).toEqual({ name: 'go', surfaceId: 's1', sourceComponentId: 'go', timestamp: '2026-01-02T03:04:05.000Z', context: { who: 'Ada', n: 2 } })
  })

  test('a button that only calls a function of the page tells the agent nothing', () => {
    const [surface] = surfacesOf(V09)
    const local: Component = { id: 'b', component: 'Button', action: { functionCall: { call: 'openUrl', args: { url: 'https://example.com' } } } }
    expect(userAction(surface, local, surface.data, '', new Date())).toBeNull()
  })
})

test('media is loaded from the web or from the URL itself, and from nowhere else', () => {
  expect(mediaUrl('https://example.com/a.png')).toBe('https://example.com/a.png')
  expect(mediaUrl('data:image/png;base64,AAAA')).toBe('data:image/png;base64,AAAA')
  expect(mediaUrl('javascript:alert(1)')).toBeUndefined()
  expect(mediaUrl('file:///etc/passwd')).toBeUndefined()
  expect(mediaUrl({ path: '/x' })).toBeUndefined()
})

describe('what an agent cannot make the window do', () => {
  test('a list is not grown far past its end', () => {
    expect(withValueAt({ a: [1] }, '/a/5000', 2)).toEqual({ a: [1] })
    expect(withValueAt({ a: [1] }, '/a/1', 2)).toEqual({ a: [1, 2] })
  })

  test('a path deeper than data goes leaves the data alone', () => {
    expect(withValueAt({ k: 1 }, '/a'.repeat(40), 1)).toEqual({ k: 1 })
  })

  test('a surface whose few components ask for a drawing without end is not drawn', () => {
    // each column draws the next one twice: 2^20 texts from 21 components
    const columns = Array.from({ length: 20 }, (_, i) => ({
      id: i === 0 ? 'root' : `c${i}`,
      component: 'Column',
      children: [`c${i + 1}`, `c${i + 1}`]
    }))
    const components = [...columns, { id: 'c20', component: 'Text', text: 'x' }]
    const [surface] = surfacesOf({
      a2ui_operations: [{ createSurface: { surfaceId: 's' } }, { updateComponents: { surfaceId: 's', components } }]
    })
    expect(drawable(surface, surface.data)).toBe(false)
  })

  test('a child named by something other than text is counted as it is drawn', () => {
    // a row of 200 cards, each holding the next row under a number: the drawing reads the number as an id
    const rows = Array.from({ length: 6 }, (_, k) => ({ id: k === 0 ? 'root' : String(k), component: 'Row', children: Array(200).fill(`card${k}`) }))
    const cards = Array.from({ length: 6 }, (_, k) => ({ id: `card${k}`, component: 'Card', child: k + 1 }))
    const [surface] = surfacesOf({
      a2ui_operations: [{ createSurface: { surfaceId: 's' } }, { updateComponents: { surfaceId: 's', components: [...rows, ...cards] } }]
    })
    expect(drawable(surface, surface.data)).toBe(false)
  })

  test('tabs and options are the same few for the count and for the drawing', () => {
    const tabs = [...Array(300).fill(1), ...Array.from({ length: 300 }, (_, i) => ({ title: `T${i}`, child: `c${i}` }))]
    const options = Array.from({ length: 5000 }, (_, i) => ({ value: `v${i}` }))
    expect(tabsOf({ id: 't', component: 'Tabs', tabs }, {}, '')).toHaveLength(200)
    expect(tabsOf({ id: 't', component: 'Tabs', tabs }, {}, '')[0]).toEqual({ title: 'T0', child: 'c0' })
    expect(optionsOf({ id: 'o', component: 'ChoicePicker', options }, {}, '')).toHaveLength(200)
  })

  test('a text of nothing but references reads the data a few times, and never writes a list into a sentence', () => {
    const data = { name: 'Ada', all: [1, 2, 3] }
    const many = { call: 'formatString', args: { value: '${/name}'.repeat(500) } }
    expect(textOf(many, data)).toBe('Ada'.repeat(64))
    expect(textOf({ call: 'formatString', args: { value: 'x${/all}y' } }, data)).toBe('xy')
  })

  test('an ordinary surface is drawn', () => {
    const [surface] = surfacesOf(V09)
    expect(drawable(surface, surface.data)).toBe(true)
  })
})
