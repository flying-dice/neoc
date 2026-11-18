/**
 * The view trees of the "Renderer/Kit - controls" stories: JSON, the way the host sends it, one scenario per area
 * (a form of every control, a two-way editor, presentations, menus, Markdown, drag and drop, a toast). Used by the
 * stories and by the renderer tests, so a regression in an adapter shows in both.
 */
import type { Action, Json, Node } from '../protocol'
import type { KitAppOptions } from './kitApp'

const text = (key: string, props: Node['props']): Node => ({ type: 'Text', key, props })
const stack = (key: string, children: Node[], props: Node['props'] = {}): Node => ({
  type: 'Stack',
  key,
  props: { axis: 'v', spacing: 3, ...props },
  children
})
const screen = (title: string, children: Node[]): Node => ({
  type: 'Screen',
  key: 'main',
  props: { title },
  children: [stack('_0', children, { padding: 4 })]
})
const set = (ref: string, value: boolean | string): Action => ({ do: 'set', ref, value })
const button = (
  key: string,
  title: string,
  tap: Action[] | true,
  props: Node['props'] = {}
): Node => ({
  type: 'Button',
  key,
  props: { title, ...props },
  on: { tap }
})

const OPTIONS: Json[] = [
  { value: 'title', label: 'Title', icon: 'doc' },
  { value: 'date', label: 'Date edited', icon: 'clock' },
  { value: 'size', label: 'Size', disabled: true }
]

/** A form of every control, two-way bound to state, with a readout that proves the values arrive. */
export const controlsForm: KitAppOptions = {
  state: {
    name: { value: 'Ada' },
    secret: { value: '' },
    notify: { value: true },
    bold: { value: false },
    agree: { value: false },
    volume: { value: 0.4 },
    count: { value: 3 },
    sort: { value: 'title' },
    tags: { value: ['a'] },
    when: { value: '2026-11-18' },
    at: { value: '09:30' },
    start: { value: '2026-11-18T09:30:00Z' },
    color: { value: 'blue' }
  },
  client: { search: { init: '' } },
  tree: screen('Controls', [
    {
      type: 'TextField',
      key: 'name',
      props: { label: 'Name', placeholder: 'Your name', text: { $ref: 'state.name' } }
    },
    {
      type: 'SecureField',
      key: 'secret',
      props: { label: 'Passphrase', text: { $ref: 'state.secret' } }
    },
    {
      type: 'SearchField',
      key: 'search',
      props: { placeholder: 'Search', text: { $ref: 'client.search' } }
    },
    {
      type: 'Toggle',
      key: 'notify',
      props: { title: 'Notifications', on: { $ref: 'state.notify' } }
    },
    {
      type: 'Toggle',
      key: 'bold',
      props: { title: 'Bold', style: 'button', on: { $ref: 'state.bold' } }
    },
    {
      type: 'Toggle',
      key: 'agree',
      props: { title: 'I agree', style: 'checkbox', on: { $ref: 'state.agree' } }
    },
    {
      type: 'Slider',
      key: 'volume',
      props: {
        label: 'Volume',
        value: { $ref: 'state.volume' },
        min: 0,
        max: 1,
        step: 0.1,
        ticks: 11,
        minLabel: 'Quiet',
        maxLabel: 'Loud'
      }
    },
    {
      type: 'Stepper',
      key: 'count',
      props: { title: 'Guests', value: { $ref: 'state.count' }, min: 0, max: 12 }
    },
    {
      type: 'Picker',
      key: 'sort-seg',
      props: {
        label: 'Sort',
        style: 'segmented',
        selection: { $ref: 'state.sort' },
        options: OPTIONS.slice(0, 2)
      }
    },
    {
      type: 'Picker',
      key: 'sort-menu',
      props: {
        label: 'Sort by',
        style: 'menu',
        selection: { $ref: 'state.sort' },
        options: OPTIONS
      }
    },
    {
      type: 'Picker',
      key: 'sort-inline',
      props: {
        label: 'Sort order',
        style: 'inline',
        selection: { $ref: 'state.sort' },
        options: OPTIONS
      }
    },
    {
      type: 'Picker',
      key: 'tags',
      props: {
        label: 'Tags',
        style: 'menu',
        multiple: true,
        selection: { $ref: 'state.tags' },
        options: [
          { value: 'a', label: 'Alpha' },
          { value: 'b', label: 'Beta' },
          { value: 'c', label: 'Gamma' }
        ]
      }
    },
    {
      type: 'DatePicker',
      key: 'when',
      props: { label: 'Due date', value: { $ref: 'state.when' } }
    },
    {
      type: 'DatePicker',
      key: 'at',
      props: { label: 'Reminder', kind: 'time', step: 15, value: { $ref: 'state.at' } }
    },
    {
      type: 'DatePicker',
      key: 'start',
      props: {
        label: 'Starts',
        kind: 'dateTime',
        style: 'graphical',
        value: { $ref: 'state.start' }
      }
    },
    {
      type: 'ColorPicker',
      key: 'color',
      props: { label: 'Label colour', value: { $ref: 'state.color' } }
    },
    text('summary', {
      text: {
        $fn: 'concat',
        args: [
          { $ref: 'state.name' },
          ' | ',
          { $ref: 'state.volume' },
          ' | ',
          { $ref: 'state.count' },
          ' | ',
          { $ref: 'state.sort' },
          ' | ',
          { $ref: 'state.when' },
          ' | ',
          { $ref: 'state.color' }
        ]
      },
      style: 'footnote',
      foreground: 'secondary'
    }),
    button('save', 'Save', true, { role: 'primary' }),
    { type: 'Shortcut', key: 'quick-find', props: { shortcut: 'cmd+k' }, on: { trigger: true } }
  ])
}

/** The display components: text styles, label, icon, image, badge, progress, markdown, avatar. */
export const display: KitAppOptions = {
  tree: screen('Display', [
    button('refresh', 'Refresh', true, { style: 'gray', size: 'small' }),
    text('title', { text: 'Large title', style: 'title2', heading: 2 }),
    text('ok', { text: 'Saved', foreground: 'success', style: 'footnote' }),
    text('warn', { text: 'Disk almost full', foreground: 'warning', style: 'footnote' }),
    text('clamped', {
      text: 'A long passage that should wrap and then be cut off after two lines with an ellipsis at the end of it all',
      lines: 2
    }),
    { type: 'Label', key: 'label', props: { title: 'Archive', icon: 'folder' } },
    stack(
      '_1',
      [
        { type: 'Icon', key: 'icon', props: { name: 'star', size: 24, label: 'Starred' } },
        { type: 'Badge', key: 'count', props: { value: 120, max: 99, label: '99 plus unread' } },
        { type: 'Badge', key: 'dot', props: { label: 'Unread' } },
        { type: 'Avatar', key: 'ada', props: { name: 'Ada Lovelace', status: 'online' } },
        { type: 'Avatar', key: 'navy', props: { name: 'Linus T', tint: '#1d3557', size: 'lg' } },
        {
          type: 'Image',
          key: 'cover',
          props: {
            src: { file: 'cover.png' },
            alt: 'Project cover',
            placeholder: 'Cover',
            size: 56
          }
        }
      ],
      { axis: 'h', spacing: 3, wrap: true }
    ),
    { type: 'ProgressView', key: 'bar', props: { value: 0.35, label: 'Upload' } },
    { type: 'ProgressView', key: 'wait', props: { label: 'Preparing' } },
    stack(
      '_2',
      [
        {
          type: 'ProgressView',
          key: 'ring',
          props: { style: 'circular', value: 0.6, label: 'Sync' }
        },
        { type: 'ProgressView', key: 'spin', props: { style: 'spinner', label: 'Loading' } }
      ],
      { axis: 'h', spacing: 3 }
    )
  ])
}

/** A two-way editor with drafts, and a Markdown preview bound to the same ref (it follows every keystroke). */
export const editor: KitAppOptions = {
  state: { body: { value: '# Groceries\n\n- milk\n- eggs\n' } },
  client: { sel: { init: null } },
  tree: screen('Editor', [
    {
      type: 'TextEditor',
      key: 'body',
      props: {
        label: 'Body',
        placeholder: 'Write a note',
        text: { $ref: 'state.body' },
        selection: { $ref: 'client.sel' }
      }
    },
    text('preview-title', { text: 'Preview', style: 'footnote', foreground: 'secondary' }),
    { type: 'Markdown', key: 'preview', props: { text: { $ref: 'state.body' } } },
    text('length', {
      text: {
        $fn: 'concat',
        args: [{ $fn: 'len', args: [{ $ref: 'state.body' }] }, ' characters']
      },
      style: 'footnote'
    })
  ])
}

/** Sheet, Alert (and a prompt), ConfirmationDialog and Popover, opened and closed by client state. */
export const presentations: KitAppOptions = {
  client: {
    sheet: { init: false },
    alert: { init: false },
    prompt: { init: false },
    confirm: { init: false },
    popover: { init: false }
  },
  tree: screen('Presentations', [
    text('last', { text: { $ref: 'state.last' }, style: 'footnote', foreground: 'secondary' }),
    stack(
      '_1',
      [
        button('open-sheet', 'Sheet', [set('client.sheet', true)]),
        button('open-alert', 'Alert', [set('client.alert', true)]),
        button('open-prompt', 'Prompt', [set('client.prompt', true)]),
        button('open-confirm', 'Confirm', [set('client.confirm', true)]),
        button('open-popover', 'Popover', [set('client.popover', true)])
      ],
      { axis: 'h', spacing: 2, wrap: true }
    ),
    {
      type: 'Group',
      key: '_2',
      children: [
        {
          type: 'Sheet',
          key: 'sheet',
          props: {
            presented: { $ref: 'client.sheet' },
            label: 'New folder',
            style: 'page',
            detents: ['medium', 'large']
          },
          children: [
            stack(
              '_3',
              [
                text('title', { text: 'New folder', style: 'headline' }),
                {
                  type: 'TextField',
                  key: 'name',
                  props: {
                    label: 'Folder name',
                    placeholder: 'Folder name',
                    text: { $ref: 'state.folder' }
                  }
                },
                button(
                  'done',
                  'Done',
                  [set('client.sheet', false), { do: 'emit', event: 'folder-created' }],
                  { role: 'primary' }
                ),
                button('cancel', 'Cancel', [set('client.sheet', false)], { role: 'cancel' })
              ],
              { padding: 4 }
            )
          ]
        },
        {
          type: 'Alert',
          key: 'alert',
          props: {
            presented: { $ref: 'client.alert' },
            title: 'Delete this note?',
            message: 'It will be removed from every device.'
          },
          slots: {
            actions: [
              button('alert-cancel', 'Cancel', [], { role: 'cancel' }),
              button('alert-delete', 'Delete', [{ do: 'emit', event: 'deleted' }], {
                role: 'destructive'
              })
            ]
          }
        },
        {
          type: 'Alert',
          key: 'prompt',
          props: {
            presented: { $ref: 'client.prompt' },
            title: 'Rename note',
            message: 'Enter a new name.',
            fields: [{ key: 'name', placeholder: 'Name' }]
          },
          slots: {
            actions: [
              button('prompt-cancel', 'Cancel', [], { role: 'cancel' }),
              button('prompt-rename', 'Rename', true, { role: 'primary' })
            ]
          }
        },
        {
          type: 'ConfirmationDialog',
          key: 'confirm',
          props: {
            presented: { $ref: 'client.confirm' },
            title: 'Discard draft?',
            message: 'This cannot be undone.',
            anchor: 'open-confirm'
          },
          slots: {
            actions: [
              button('confirm-discard', 'Discard draft', [{ do: 'emit', event: 'discarded' }], {
                role: 'destructive'
              }),
              button('confirm-keep', 'Keep editing', [], { role: 'cancel' })
            ]
          }
        },
        {
          type: 'Popover',
          key: 'popover',
          props: {
            presented: { $ref: 'client.popover' },
            anchor: 'open-popover',
            label: 'Filters'
          },
          children: [
            stack(
              '_4',
              [
                text('popover-title', { text: 'Filters', style: 'headline' }),
                {
                  type: 'Toggle',
                  key: 'popover-pinned',
                  props: { title: 'Pinned only', on: { $ref: 'state.pinned' } }
                },
                button('popover-close', 'Close', [set('client.popover', false)])
              ],
              { padding: 3 }
            )
          ]
        }
      ]
    }
  ]),
  state: { folder: { value: '' }, pinned: { value: false }, last: { value: 'nothing yet' } },
  onEvent: (_view, event, host) => {
    const leaf = event.path.split('/').slice(-1)[0]
    host.setState(
      'last',
      `${event.event} ${leaf}${event.payload ? ` ${JSON.stringify(event.payload)}` : ''}`
    )
  }
}

/** A menu with every item kind, and a node with a context menu. */
export const menus: KitAppOptions = {
  state: { pinned: { value: true }, sort: { value: 'title' }, last: { value: 'nothing yet' } },
  tree: screen('Menus', [
    text('last', { text: { $ref: 'state.last' }, style: 'footnote', foreground: 'secondary' }),
    {
      type: 'Menu',
      key: 'actions',
      props: { title: 'Actions' },
      children: [
        {
          type: 'Button',
          key: 'new',
          props: { title: 'New note', icon: 'square.and.pencil', shortcut: 'cmd+n' },
          on: { tap: true }
        },
        { type: 'Toggle', key: 'pin', props: { title: 'Pinned', on: { $ref: 'state.pinned' } } },
        { type: 'Divider', key: 'd1' },
        {
          type: 'Picker',
          key: 'sort',
          props: {
            label: 'Sort by',
            style: 'inline',
            selection: { $ref: 'state.sort' },
            options: OPTIONS
          }
        },
        {
          type: 'Section',
          key: 'move-section',
          props: { header: 'Organise' },
          children: [
            {
              type: 'Menu',
              key: 'move',
              props: { title: 'Move to' },
              children: [
                { type: 'Button', key: 'inbox', props: { title: 'Inbox' }, on: { tap: true } },
                { type: 'Button', key: 'archive', props: { title: 'Archive' }, on: { tap: true } }
              ]
            },
            {
              type: 'Button',
              key: 'delete',
              props: { title: 'Delete', icon: 'trash', role: 'destructive' },
              on: { tap: true }
            }
          ]
        }
      ]
    },
    {
      type: 'Menu',
      key: 'split',
      props: { title: 'Share', icon: 'square.and.arrow.up' },
      on: { primaryAction: true },
      children: [
        { type: 'Button', key: 'copy-link', props: { title: 'Copy link' }, on: { tap: true } }
      ]
    },
    {
      type: 'Button',
      key: 'card',
      props: { title: 'Right-click, long-press or Shift+F10 me', style: 'gray' },
      slots: {
        contextMenu: [
          {
            type: 'Button',
            key: 'rename',
            props: { title: 'Rename', icon: 'square.and.pencil' },
            on: { tap: true }
          },
          {
            type: 'Toggle',
            key: 'star',
            props: { title: 'Starred', on: { $ref: 'state.pinned' } }
          },
          { type: 'Divider', key: 'cd' },
          {
            type: 'Button',
            key: 'remove',
            props: { title: 'Remove', icon: 'trash', role: 'destructive' },
            on: { tap: true }
          }
        ]
      }
    }
  ]),
  onEvent: (_view, event, host) => {
    host.setState('last', `${event.path.split('/').slice(-1)[0]} ${event.event}`)
  }
}

export const markdownSample = [
  '# Release notes',
  '',
  'AgentOS **1.0** adds _tables_ and [docs](https://example.com/docs).',
  '',
  '- [x] Parser',
  '- [ ] Ship it',
  '',
  '> Keep the text, drop the markup.',
  '',
  '```ts',
  'const greet = (n: string) => `hello ${n}`',
  '```',
  '',
  '| Tool | Calls |',
  '|:-----|------:|',
  '| read | 12 |',
  '',
  '<script>alert(1)</script> and [bad](javascript:alert(1))'
].join('\n')

export const markdown: KitAppOptions = {
  state: { last: { value: '' } },
  tree: screen('Markdown', [
    { type: 'Markdown', key: 'doc', props: { text: markdownSample, selectable: true } }
  ])
}

/** Drag and drop: three columns that accept tasks, and cards that can be dragged. */
export const dnd: KitAppOptions = {
  state: { last: { value: 'nothing moved yet' } },
  tree: screen('Board', [
    text('last', { text: { $ref: 'state.last' }, style: 'footnote' }),
    stack(
      '_1',
      ['todo', 'doing', 'done'].map((column): Node => ({
        type: 'Stack',
        key: column,
        props: {
          axis: 'v',
          spacing: 2,
          padding: 2,
          label: column === 'todo' ? 'To do' : column === 'doing' ? 'Doing' : 'Done',
          drop: { accepts: ['task'], highlight: true },
          background: 'fill4',
          corner: 'md'
        },
        on: { drop: true },
        children: [
          text(`${column}-title`, {
            text: column === 'todo' ? 'To do' : column === 'doing' ? 'Doing' : 'Done',
            style: 'footnote',
            weight: 'semibold'
          }),
          ...(column === 'todo'
            ? ['a', 'b'].map((id): Node => ({
                type: 'Button',
                key: `card-${id}`,
                props: {
                  title: `Task ${id.toUpperCase()}`,
                  style: 'gray',
                  drag: { type: 'task', payload: { id } }
                },
                on: { tap: true }
              }))
            : [])
        ]
      })),
      { axis: 'h', spacing: 2 }
    )
  ]),
  onEvent: (_view, event, host) => {
    if (event.event === 'drop') {
      const p = event.payload as {
        payload?: { id?: string }
        target?: { key?: string; edge?: string }
      } | null
      host.setState(
        'last',
        `moved ${p?.payload?.id ?? '?'} to ${p?.target?.key ?? '?'} (${p?.target?.edge ?? '?'})`
      )
    }
  }
}

/** A button whose event makes the host send a toast with an action; tapping the action reports the `toast` event. */
export const toast: KitAppOptions = {
  state: { last: { value: 'no toast yet' } },
  tree: screen('Toast', [
    text('last', { text: { $ref: 'state.last' } }),
    button('delete', 'Delete note', true)
  ]),
  onEvent: (view, event, host) => {
    if (event.event === 'tap')
      view.command({
        cmd: 'toast',
        args: { text: 'Note deleted', kind: 'success', action: 'Undo', duration: 0 }
      } as never)
    if (event.event === 'toast') host.setState('last', 'undo tapped')
  }
}
