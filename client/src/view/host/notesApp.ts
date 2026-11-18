/**
 * "Notes-lite": the demo window the fake host serves in unit tests and the Renderer stories. It exercises every
 * piece of the starter kit and every protocol path: a full tree, keyed ops (status text, the empty state), a
 * two-way text binding on app state (`state.body`) and on a row field (`src.note.body`), a scalar binding in the
 * rows of a list (`row.pinned`), a source parameterised by client state (`client.search`), a windowed list, and
 * agent writes that arrive while the user is typing.
 */
import type { FakeApp, FakeView } from './fakeHost'
import { FakeHost } from './fakeHost'
import type { ClientDecl, Json, Node, Row, ViewEventParams } from '../protocol'

export interface Note {
  id: number
  title: string
  body: string
  pinned: boolean
  rev: number
}

export interface NotesOptions {
  /** How many generated notes to add after the two named ones (a windowed list needs more than a page). */
  extra?: number
  /** Page size of the `notes` source. */
  page?: number
  /** `reject` makes the `approved` toggle strict: a stale write is refused instead of overwriting. */
  approvedMerge?: 'scalar' | 'reject'
  /** Extra nodes appended to the content stack (an unknown component and its fallback, for the stories). */
  extraNodes?: Node[]
}

const titleOf = (body: string): string => body.split('\n')[0].slice(0, 40) || 'New note'

function seed(extra: number): Note[] {
  const notes: Note[] = [
    { id: 1, title: 'Groceries', body: 'milk, eggs', pinned: true, rev: 1 },
    { id: 2, title: 'Ideas', body: 'server-driven UI', pinned: false, rev: 1 }
  ]
  for (let i = 0; i < extra; i++)
    notes.push({
      id: 3 + i,
      title: `Note ${3 + i}`,
      body: `Generated note ${3 + i}`,
      pinned: false,
      rev: 1
    })
  return notes
}

const emptyNode = (): Node => ({
  type: 'Text',
  key: 'empty-state',
  props: { text: 'No notes yet', style: 'callout', foreground: 'secondary' }
})

export class NotesApp implements FakeApp {
  readonly id = 'notes'
  notes: Note[]
  private nextId: number
  private emptyShown = false

  readonly clientDecls: Record<string, ClientDecl> = {
    search: { init: '', persist: 'session' },
    noteId: { init: 1, persist: 'app' }
  }

  readonly state: FakeApp['state']
  readonly sources: FakeApp['sources']

  constructor(private o: NotesOptions = {}) {
    this.notes = seed(o.extra ?? 0)
    this.nextId = this.notes.length + 1
    this.state = {
      body: { value: '', merge: 'text' },
      approved: { value: false, merge: o.approvedMerge ?? 'scalar' }
    }
    const sorted = (): Note[] => [...this.notes].sort((a, b) => b.id - a.id)
    const rowOf = (n: Note): Row => ({
      id: n.id,
      title: n.title,
      preview: n.body.slice(0, 60),
      pinned: n.pinned,
      rev: n.rev
    })
    this.sources = {
      notes: {
        decl: {
          kind: 'rows',
          key: 'id',
          page: o.page ?? 60,
          debounce: 150,
          params: { q: { $ref: 'client.search' } }
        },
        query: (p) => {
          const q = String(p.q ?? '').toLowerCase()
          return sorted()
            .filter(
              (n) => !q || n.title.toLowerCase().includes(q) || n.body.toLowerCase().includes(q)
            )
            .map(rowOf)
        },
        find: (key) => {
          const n = this.notes.find((x) => x.id === Number(key))
          return n ? rowOf(n) : null
        },
        merge: 'scalar',
        write: (key, field, value) => {
          const n = this.notes.find((x) => x.id === Number(key))
          if (!n || field !== 'pinned') throw new Error(`cannot write ${field}`)
          n.pinned = Boolean(value)
          return ++n.rev
        }
      },
      note: {
        decl: {
          kind: 'row',
          key: 'id',
          writable: ['body', 'pinned'],
          params: { id: { $ref: 'client.noteId' } }
        },
        get: (p) => {
          const n = this.notes.find((x) => x.id === Number(p.id))
          return n ? { id: n.id, body: n.body, pinned: n.pinned, rev: n.rev } : null
        },
        merge: 'text',
        write: (key, field, value) => {
          const n = this.notes.find((x) => x.id === Number(key))
          if (!n) throw new Error(`no note ${String(key)}`)
          if (field === 'pinned') n.pinned = Boolean(value)
          else if (field === 'body') {
            n.body = String(value)
            n.title = titleOf(n.body)
          } else throw new Error(`cannot write ${field}`)
          return ++n.rev
        }
      }
    }
  }

  reset(): void {
    this.notes = seed(this.o.extra ?? 0)
    this.nextId = this.notes.length + 1
  }

  tree(): Node {
    this.emptyShown = this.notes.length === 0
    return {
      type: 'Screen',
      key: 'main',
      props: {
        title: 'Notes',
        titleDisplay: 'large',
        searchable: { text: { $ref: 'client.search' }, placeholder: 'Search notes' }
      },
      children: [
        {
          type: 'Stack',
          key: '_0',
          props: { axis: 'v', spacing: 3, padding: 4 },
          children: [
            {
              type: 'Text',
              key: 'count',
              props: {
                text: {
                  $fn: 'fmt.plural',
                  args: [
                    { $ref: 'src.notes.total' },
                    { $lit: { one: '{n} note', other: '{n} notes' } }
                  ]
                },
                style: 'footnote',
                foreground: 'secondary'
              }
            },
            {
              type: 'List',
              key: 'list',
              props: {
                source: 'notes',
                label: 'Notes',
                style: 'insetGrouped',
                selection: { $ref: 'client.noteId' }
              },
              slots: {
                row: [
                  {
                    type: 'Row',
                    key: 'item',
                    props: {
                      title: { $ref: 'row.title' },
                      subtitle: { $ref: 'row.preview' },
                      tag: { $ref: 'row.id' },
                      value: { $fn: 'if', args: [{ $ref: 'row.pinned' }, 'Pinned', ''] }
                    }
                  }
                ]
              }
            },
            ...(this.emptyShown ? [emptyNode()] : []),
            {
              type: 'TextField',
              key: 'body',
              props: {
                text: { $ref: 'state.body' },
                placeholder: 'Write a note…',
                label: 'New note'
              }
            },
            {
              type: 'TextField',
              key: 'note-body',
              props: {
                text: { $ref: 'src.note.body' },
                placeholder: 'Selected note',
                label: 'Selected note'
              }
            },
            {
              type: 'Toggle',
              key: 'pinned',
              props: { on: { $ref: 'src.note.pinned' }, title: 'Pin selected note' }
            },
            {
              type: 'Toggle',
              key: 'approved',
              props: { on: { $ref: 'state.approved' }, title: 'Approved' }
            },
            {
              type: 'Stack',
              key: 'actions',
              props: { axis: 'h', spacing: 2, wrap: true },
              children: [
                {
                  type: 'Button',
                  key: 'save',
                  props: { title: 'Save as note', style: 'filled' },
                  on: { tap: true }
                },
                {
                  type: 'Button',
                  key: 'delete-all',
                  props: { title: 'Delete all', style: 'gray', role: 'destructive' },
                  on: { tap: true }
                },
                {
                  type: 'Button',
                  key: 'clear',
                  props: { title: 'Clear search', style: 'gray' },
                  on: { tap: [{ do: 'set', ref: 'client.search', value: '' }] }
                }
              ]
            },
            {
              type: 'Text',
              key: 'status',
              props: { text: 'Ready', style: 'footnote', foreground: 'secondary' }
            },
            ...(this.o.extraNodes ?? [])
          ]
        }
      ]
    }
  }

  async onEvent(view: FakeView, e: ViewEventParams): Promise<void> {
    const host = view.host
    if (e.path === 'main/_0/actions/save' && e.event === 'tap') {
      const body = String(host.stateValue('body')).trim()
      if (!body) return this.status(view, 'Nothing to save')
      this.notes.push({ id: this.nextId++, title: titleOf(body), body, pinned: false, rev: 1 })
      host.setState('body', '')
      host.refreshSources()
      this.structure(view)
      return this.status(view, `Saved note ${this.nextId - 1}`)
    }
    if (e.path === 'main/_0/actions/delete-all' && e.event === 'tap') {
      this.notes = []
      host.refreshSources()
      this.structure(view)
      return this.status(view, 'Deleted all notes')
    }
    throw new Error(`unhandled event ${e.event} on ${e.path}`)
  }

  /** Show or hide the empty state: a structural change, so it travels as keyed ops, not data. */
  private structure(view: FakeView): void {
    const empty = this.notes.length === 0
    if (empty === this.emptyShown) return
    this.emptyShown = empty
    view.ops(
      empty
        ? [{ op: 'insert', parent: 'main/_0', before: 'body', node: emptyNode() }]
        : [{ op: 'remove', path: 'main/_0/empty-state' }]
    )
  }

  private status(view: FakeView, text: string): void {
    view.ops([{ op: 'set', path: 'main/_0/status', props: { text } }])
  }

  // ── "Agent tools": change data while a view is open ────────────────────

  /** `notes.append` on the open draft: the view updates through its binding, not a re-render. */
  agentAppendDraft(host: FakeHost, text: string): void {
    host.setState('body', String(host.stateValue('body')) + text)
  }

  /** `notes.append` on a stored note. */
  agentAppendNote(host: FakeHost, id: number, text: string): void {
    const n = this.notes.find((x) => x.id === id)
    if (!n) return
    n.body += text
    n.rev++
    host.refreshSources()
  }

  agentAddNote(host: FakeHost, title: string): number {
    const id = this.nextId++
    this.notes.push({ id, title, body: 'Added by an agent', pinned: false, rev: 1 })
    host.refreshSources()
    for (const v of host.allViews()) this.structure(v)
    return id
  }
}

export type { Json }
