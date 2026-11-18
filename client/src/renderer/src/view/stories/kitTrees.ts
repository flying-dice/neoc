/**
 * The JSON view trees of the "Renderer/Kit — layout" stories: what a Luau app's `render` would send, written out. Each
 * builder returns a `TreeApp` (the fake host app) so a story can open it and its play function can drive the host.
 */
import type { Node, Row, RowKey } from '../protocol'
import { generatedRows, TreeApp } from '../host/treeApp'

const text = (key: string, value: string, extra: Node['props'] = {}): Node => ({
  type: 'Text',
  key,
  props: { text: value, ...extra }
})

const row = (
  key: string,
  title: string,
  extra: Node['props'] = {},
  rest: Partial<Node> = {}
): Node => ({
  type: 'Row',
  key,
  props: { title, ...extra },
  ...rest
})

export type ListStyle = 'plain' | 'inset' | 'insetGrouped' | 'grouped' | 'sidebar'

/** One screen: a List in each style, with two sections, a selected row, a chevron row and a value row. */
export function listStyleApp(style: ListStyle): TreeApp {
  const section = (key: string, header: string, rows: Node[]): Node => ({
    type: 'Section',
    key,
    props: { header },
    children: rows
  })
  return new TreeApp({
    client: { picked: { init: 'wifi' } },
    tree: {
      type: 'Screen',
      key: 'main',
      props: {
        title: 'Settings',
        titleDisplay: 'inline',
        background: style === 'plain' || style === 'inset' ? 'plain' : 'grouped'
      },
      children: [
        {
          type: 'List',
          key: 'list',
          props: { style, selection: { $ref: 'client.picked' }, label: 'Settings' },
          children: [
            section('network', 'Network', [
              row('wifi', 'Wi-Fi', { tag: 'wifi', value: 'Home', icon: 'wifi' }),
              row('bluetooth', 'Bluetooth', { tag: 'bluetooth', value: 'On', icon: 'network' }),
              row('vpn', 'VPN', { tag: 'vpn', chevron: true })
            ]),
            section('general', 'General', [
              row('about', 'About', { tag: 'about', subtitle: 'Version 1.0', chevron: true }),
              row('storage', 'Storage', { tag: 'storage', value: '12 GB', chevron: true }),
              row('reset', 'Reset…', { tag: 'reset', role: 'destructive' })
            ])
          ]
        }
      ]
    }
  })
}

/** A source of `n` rows in a List: tap, selection, windows. */
export function longListApp(
  n: number,
  extra: { style?: ListStyle; page?: number; refresh?: boolean } = {}
): TreeApp {
  return new TreeApp({
    client: { picked: { init: 3 } },
    sources: { items: { rows: generatedRows(n), page: extra.page ?? 60 } },
    tree: {
      type: 'Screen',
      key: 'main',
      props: { title: `${n.toLocaleString('en')} items`, titleDisplay: 'large' },
      children: [
        {
          type: 'List',
          key: 'list',
          props: {
            source: 'items',
            style: extra.style ?? 'plain',
            selection: { $ref: 'client.picked' },
            label: 'Items'
          },
          ...(extra.refresh ? { on: { refresh: true } } : {}),
          slots: {
            row: [
              row('item', '', {
                title: { $ref: 'row.title' },
                subtitle: { $ref: 'row.subtitle' },
                tag: { $ref: 'row.id' }
              })
            ]
          }
        }
      ]
    },
    // A refresh takes a moment: the spinner runs until the app answers.
    onEvent: (_app, _view, e) =>
      e.event === 'refresh' ? new Promise((resolve) => setTimeout(resolve, 400)) : undefined
  })
}

/** Rows with trailing swipe actions (Delete, Pin) and a leading one (Read): the host deletes on `tap`. */
export function swipeApp(): TreeApp {
  const rows: Row[] = ['Groceries', 'Ideas', 'Trip plan', 'Reading list', 'Recipes'].map(
    (title, i) => ({
      id: i + 1,
      title,
      subtitle: `Note ${i + 1}`
    })
  )
  return new TreeApp({
    sources: { notes: { rows } },
    tree: {
      type: 'Screen',
      key: 'main',
      props: { title: 'Notes', titleDisplay: 'large' },
      children: [
        {
          type: 'List',
          key: 'list',
          props: { source: 'notes', style: 'insetGrouped', label: 'Notes' },
          slots: {
            row: [
              {
                type: 'Row',
                key: 'item',
                props: { title: { $ref: 'row.title' }, subtitle: { $ref: 'row.subtitle' } },
                on: { tap: true },
                slots: {
                  swipeTrailing: [
                    {
                      type: 'Button',
                      key: 'delete',
                      props: {
                        title: 'Delete',
                        icon: 'trash',
                        role: 'destructive',
                        fullSwipe: true
                      },
                      on: { tap: true }
                    },
                    {
                      type: 'Button',
                      key: 'pin',
                      props: { title: 'Pin', icon: 'star' },
                      on: { tap: true }
                    }
                  ],
                  swipeLeading: [
                    {
                      type: 'Button',
                      key: 'read',
                      props: { title: 'Read', role: 'cancel' },
                      on: { tap: true }
                    }
                  ]
                }
              }
            ]
          }
        }
      ]
    },
    onEvent: (app, view, e) => {
      if (e.event === 'tap' && e.path.endsWith('/delete') && e.rowKey !== undefined) {
        app.remove('notes', e.rowKey as RowKey)
        view.host.refreshSources()
      }
    }
  })
}

/** A list that reorders: `onMove {key, beforeKey}` reaches the app, which stores the new order. */
export function reorderApp(): TreeApp {
  const rows: Row[] = ['Milk', 'Eggs', 'Bread', 'Cheese', 'Apples', 'Tea'].map((title, i) => ({
    id: i + 1,
    title
  }))
  return new TreeApp({
    client: { editing: { init: true } },
    sources: { items: { rows } },
    tree: {
      type: 'Screen',
      key: 'main',
      props: { title: 'Shopping', titleDisplay: 'inline' },
      children: [
        {
          type: 'List',
          key: 'list',
          props: {
            source: 'items',
            style: 'insetGrouped',
            label: 'Shopping',
            editing: { $ref: 'client.editing' }
          },
          on: { move: true },
          slots: { row: [row('item', '', { title: { $ref: 'row.title' } })] }
        }
      ]
    },
    onEvent: (app, view, e) => {
      if (e.event !== 'move') return
      const p = e.payload as { key: number; beforeKey?: number }
      app.move('items', p.key, p.beforeKey ?? null)
      view.host.refreshSources()
    }
  })
}

const folder = (id: string, title: string, icon: string): Row => ({ id, title, icon })

/** A mail-like three-column split: folders → notes → a note. Selection is client state, so it needs no host. */
export function splitApp(columns: 2 | 3): TreeApp {
  const notes: Row[] = [
    { id: 1, title: 'Groceries', preview: 'milk, eggs, bread', folder: 'notes' },
    { id: 2, title: 'Ideas', preview: 'server-driven UI', folder: 'notes' },
    { id: 3, title: 'Trip plan', preview: 'Lisbon in May', folder: 'notes' },
    { id: 4, title: 'Passport', preview: 'renew before June', folder: 'reminders' }
  ]
  const sidebar: Node = {
    type: 'Screen',
    key: 'folders',
    props: { title: 'Folders', titleDisplay: 'inline' },
    children: [
      {
        type: 'List',
        key: 'folder-list',
        props: {
          source: 'folders',
          style: 'sidebar',
          selection: { $ref: 'client.folder' },
          label: 'Folders'
        },
        slots: {
          row: [
            row('folder', '', {
              title: { $ref: 'row.title' },
              icon: { $ref: 'row.icon' },
              tag: { $ref: 'row.id' }
            })
          ]
        }
      }
    ]
  }
  const noteList: Node = {
    type: 'Screen',
    key: 'notes',
    props: { title: { $fn: 'upper', args: [{ $ref: 'client.folder' }] }, titleDisplay: 'inline' },
    children: [
      {
        type: 'List',
        key: 'note-list',
        props: {
          source: 'notes',
          style: 'plain',
          selection: { $ref: 'client.note' },
          label: 'Notes'
        },
        slots: {
          row: [
            row('note', '', {
              title: { $ref: 'row.title' },
              subtitle: { $ref: 'row.preview' },
              tag: { $ref: 'row.id' }
            })
          ]
        }
      }
    ]
  }
  const detail: Node = {
    type: 'Screen',
    key: 'note',
    props: { title: 'Note', titleDisplay: 'inline' },
    children: [
      {
        type: 'Stack',
        key: 'body',
        props: { padding: 4, spacing: 2 },
        children: [
          text('heading', 'Selected note', { style: 'headline' }),
          text('which', { $fn: 'concat', args: ['id ', { $ref: 'client.note' }] } as never, {
            foreground: 'secondary'
          })
        ]
      }
    ]
  }
  return new TreeApp({
    client: { folder: { init: 'notes' }, note: { init: 1 } },
    sources: {
      folders: {
        rows: [folder('notes', 'Notes', 'doc'), folder('reminders', 'Reminders', 'checkmark')]
      },
      notes: { rows: notes }
    },
    tree: {
      type: 'SplitView',
      key: 'split',
      props: { columns },
      slots:
        columns === 3
          ? { sidebar: [sidebar], content: [noteList], detail: [detail] }
          : { sidebar: [sidebar], detail: [{ ...noteList, key: 'notes' }] }
    }
  })
}

/** A navigation stack: an inbox of links pushing `note` routes; `nav.push` actions from buttons too. */
export function stackApp(): TreeApp {
  const notes: Row[] = [
    { id: 1, title: 'Groceries', body: 'milk, eggs' },
    { id: 2, title: 'Ideas', body: 'server-driven UI' },
    { id: 3, title: 'Trip plan', body: 'Lisbon in May' }
  ]
  return new TreeApp({
    client: { path: { init: [], persist: 'session' } },
    sources: { notes: { rows: notes } },
    tree: {
      type: 'NavigationStack',
      key: 'nav',
      props: { path: { $ref: 'client.path' } },
      slots: {
        root: [
          {
            type: 'Screen',
            key: 'inbox',
            props: { title: 'Inbox', titleDisplay: 'large' },
            children: [
              {
                type: 'List',
                key: 'list',
                props: { source: 'notes', style: 'insetGrouped', label: 'Notes' },
                slots: {
                  row: [
                    {
                      type: 'NavigationLink',
                      key: 'link',
                      props: {
                        title: { $ref: 'row.title' },
                        to: { name: 'note', id: { $ref: 'row.id' }, title: { $ref: 'row.title' } }
                      }
                    }
                  ]
                }
              }
            ]
          }
        ],
        'dest:note': [
          {
            type: 'Screen',
            key: 'note',
            props: { title: { $ref: 'route.title' }, titleDisplay: 'inline' },
            children: [
              {
                type: 'Stack',
                key: 'body',
                props: { padding: 4, spacing: 3 },
                children: [
                  text('id', { $fn: 'concat', args: ['Note id ', { $ref: 'route.id' }] } as never),
                  {
                    type: 'Button',
                    key: 'more',
                    props: { title: 'Open related note' },
                    on: {
                      tap: [{ do: 'nav.push', route: { name: 'note', id: 99, title: 'Related' } }]
                    }
                  },
                  {
                    type: 'Button',
                    key: 'root',
                    props: { title: 'Back to Inbox' },
                    on: { tap: [{ do: 'nav.popToRoot' }] }
                  }
                ]
              }
            ]
          }
        ]
      }
    }
  })
}

/** Everything else: stacks, grid, z-stack, adaptive, scope, card, form, disclosure, empty state, tabs. */
export function galleryApp(): TreeApp {
  const tabs: Node = {
    type: 'TabView',
    key: 'tabs',
    props: { selection: { $ref: 'client.tab' }, style: 'bottom' },
    children: [
      {
        type: 'Stack',
        key: 'layout',
        props: { tag: 'layout', padding: 4, spacing: 3 },
        slots: {
          tabItem: [{ type: 'Label', key: 'item', props: { title: 'Layout', icon: 'square.grid' } }]
        },
        children: [
          text('grid-title', 'Grid', { style: 'headline' }),
          {
            type: 'Grid',
            key: 'grid',
            props: { columns: 3, spacing: 2, cellAspect: 1.4 },
            children: [
              ...['A', 'B', 'C'].map((c) => ({
                type: 'Card',
                key: `cell-${c}`,
                props: { padding: 2 },
                children: [text(`t-${c}`, c, { style: 'title3' })]
              })),
              {
                type: 'Card',
                key: 'wide',
                props: { span: 2, background: 'tintSoft' },
                children: [text('t-wide', 'Spans two columns')]
              },
              { type: 'Card', key: 'tall', props: {}, children: [text('t-one', 'One')] }
            ]
          },
          text('z-title', 'ZStack', { style: 'headline' }),
          {
            type: 'ZStack',
            key: 'z',
            props: {
              align: 'bottomTrailing',
              frame: { minH: 80 },
              background: 'fill3',
              corner: 'md'
            },
            children: [
              text('z-back', 'Back', { style: 'title2', padding: 3 }),
              {
                type: 'Card',
                key: 'z-front',
                props: { padding: 1 },
                children: [text('z-badge', 'Front')]
              }
            ]
          },
          { type: 'Divider', key: 'rule' },
          {
            type: 'Stack',
            key: 'bar',
            props: { axis: 'h', spacing: 2, align: 'center' },
            children: [text('left', 'Left'), { type: 'Spacer', key: 'gap' }, text('right', 'Right')]
          },
          {
            type: 'Adaptive',
            key: 'adaptive',
            props: { breakpoint: 420 },
            slots: {
              compact: [
                text('compact', 'Compact layout (under 420 px)', { foreground: 'secondary' })
              ],
              regular: [
                text('regular', 'Regular layout (420 px and over)', { foreground: 'secondary' })
              ]
            }
          },
          {
            type: 'Scope',
            key: 'scope',
            props: { scheme: 'dark', padding: 3, corner: 'lg' },
            children: [
              text('scoped', 'A dark scope inside a light window', { foreground: 'primary' })
            ]
          }
        ]
      },
      {
        type: 'Form',
        key: 'form',
        props: { tag: 'form', label: 'Preferences' },
        slots: {
          tabItem: [{ type: 'Label', key: 'item', props: { title: 'Form', icon: 'gear' } }]
        },
        children: [
          {
            type: 'Section',
            key: 'account',
            props: { header: 'Account', footer: 'Used for sync.' },
            children: [
              { type: 'LabeledContent', key: 'name', props: { label: 'Name', value: 'Jonathan' } },
              {
                type: 'LabeledContent',
                key: 'plan',
                props: { label: 'Plan', value: 'Pro, billed yearly, renews on 12 March' }
              },
              {
                type: 'DisclosureGroup',
                key: 'more',
                props: { label: 'More', expanded: { $ref: 'client.more' } },
                children: [
                  {
                    type: 'LabeledContent',
                    key: 'id',
                    props: { label: 'Identifier', value: 'a1b2c3' }
                  }
                ]
              }
            ]
          }
        ]
      },
      {
        type: 'EmptyState',
        key: 'empty',
        props: {
          tag: 'empty',
          icon: 'message',
          title: 'No messages',
          message: 'New messages appear here.'
        },
        slots: {
          tabItem: [
            { type: 'Label', key: 'item', props: { title: 'Empty', icon: 'message' } },
            { type: 'Badge', key: 'count', props: { value: 3 } }
          ],
          action: [
            { type: 'Button', key: 'compose', props: { title: 'Compose' }, on: { tap: true } }
          ]
        }
      }
    ]
  }
  return new TreeApp({ client: { tab: { init: 'layout' }, more: { init: false } }, tree: tabs })
}
