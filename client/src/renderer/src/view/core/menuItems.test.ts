import { describe, expect, test } from 'bun:test'
import type { MenuEntry, MenuItem } from '@agentos/ui'
import { createHarness } from '../host/harness'
import type { Node } from '../protocol'
import { formatShortcut, iconOf, menuItems } from './menuItems'

const entry = (i: MenuItem | undefined): MenuEntry => {
  if (!i || !('label' in i)) throw new Error('expected a menu entry')
  return i
}

async function setup() {
  const h = createHarness({ latencyMs: 0 })
  const view = await h.openNotes()
  const ctx = { view, scope: {}, apple: true }
  return { h, view, ctx }
}

const button = (key: string, props: Node['props'], on?: Node['on']): Node => ({ type: 'Button', key, props, on })

describe('menu content from nodes', () => {
  test('buttons, dividers, sections and nested menus map to kit items', async () => {
    const { ctx } = await setup()
    const items = menuItems(ctx, [
      button('new', { title: 'New note', icon: 'plus', shortcut: 'cmd+n' }),
      { type: 'Divider', key: 'd' },
      { type: 'Section', key: 's', props: { header: 'Danger' }, children: [button('del', { title: 'Delete', role: 'destructive', icon: 'trash' })] },
      { type: 'Menu', key: 'move', props: { title: 'Move to' }, children: [button('inbox', { title: 'Inbox' })] }
    ], 'main')
    expect(items.map((i) => ('separator' in i ? '---' : 'heading' in i ? `# ${i.heading}` : i.label))).toEqual([
      'New note',
      '---',
      '# Danger',
      'Delete',
      'Move to'
    ])
    expect(entry(items[0])).toMatchObject({ key: 'new', icon: 'plus', shortcut: '⌘N' })
    expect(entry(items[3])).toMatchObject({ destructive: true, icon: 'trash' })
    expect(entry(items[4]).submenu).toHaveLength(1)
  })

  test('a button tap fires the node through the view core', async () => {
    const { ctx, h } = await setup()
    const items = menuItems(ctx, [button('ping', { title: 'Ping' }, { tap: true })], 'main')
    entry(items[0]).onclick?.()
    await new Promise((r) => setTimeout(r, 10))
    const event = h.host.received.find((r) => r.method === 'view.event')
    expect(event?.params).toMatchObject({ path: 'main/ping', event: 'tap' })
  })

  test('a toggle is a checkmark item that writes its binding', async () => {
    const { ctx, view } = await setup()
    const toggle: Node = { type: 'Toggle', key: 'sync', props: { title: 'Show completed', on: { $ref: 'client.noteId' } } }
    const before = menuItems(ctx, [toggle], 'main')
    expect(entry(before[0]).checked).toBe(true) // client.noteId is 1: truthy
    entry(before[0]).onclick?.()
    expect(view.resolve('client.noteId')).toBe(false)
    expect(entry(menuItems(ctx, [toggle], 'main')[0]).checked).toBe(false)
  })

  test('an inline picker lists its options with the chosen one checked', async () => {
    const { ctx, view } = await setup()
    const picker: Node = {
      type: 'Picker',
      key: 'sort',
      props: {
        label: 'Sort by',
        selection: { $ref: 'client.search' },
        options: [
          { value: 'title', label: 'Title' },
          { value: 'date', label: 'Date', disabled: true }
        ]
      }
    }
    view.write('client.search', 'title')
    const items = menuItems(ctx, [picker], 'main')
    expect(items.map((i) => ('heading' in i ? i.heading : 'label' in i ? `${i.label}:${i.checked}:${i.disabled}` : '-'))).toEqual([
      'Sort by',
      'Title:true:false',
      'Date:false:true'
    ])
    entry(items[1]).onclick?.()
    expect(view.resolve('client.search')).toBe('title')
  })

  test('Group is transparent, Show follows its condition, unknown nodes are left out', async () => {
    const { ctx } = await setup()
    const items = menuItems(ctx, [
      { type: 'Group', key: 'g', children: [button('a', { title: 'A' })] },
      { type: 'Show', key: 'show', props: { when: false }, children: [button('b', { title: 'B' })], slots: { otherwise: [button('c', { title: 'C' })] } },
      { type: 'Text', key: 't', props: { text: 'not an item' } }
    ], 'main')
    expect(items.map((i) => (i as MenuEntry).label)).toEqual(['A', 'C'])
  })
})

describe('shortcut labels and icons', () => {
  test('Apple and other platforms', () => {
    expect(formatShortcut('cmd+shift+n', true)).toBe('⇧⌘N')
    expect(formatShortcut('cmd+shift+n', false)).toBe('Ctrl+Shift+N')
    expect(formatShortcut('escape', true)).toBe('Esc')
    expect(formatShortcut('alt+down', true)).toBe('⌥↓')
    expect(formatShortcut('', true)).toBeUndefined()
  })

  test('only kit icons are used', () => {
    expect(iconOf('trash')).toBe('trash')
    expect(iconOf('nope')).toBeUndefined()
    expect(iconOf(3)).toBeUndefined()
  })
})
