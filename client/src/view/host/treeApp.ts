/**
 * A fake app that serves ANY view tree: the stories and tests of the layout, navigation and collection components
 * hand it JSON (a tree, declared client entries, row sources) and it plays the host's half: sources answer window
 * requests from their rows, events are logged and may be handled (delete a row, move one), and the story drives the
 * host side like an agent would. Story and test support only.
 */
import type { ClientDecl, Json, Node, Row, RowKey, ViewEventParams } from '../protocol'
import type { FakeApp, FakeView, SourceSpec } from './fakeHost'

export interface RowsDef {
  /** The ordered rows (the host slices windows from them). Mutate through `TreeApp.rows`. */
  rows: Row[]
  key?: string
  page?: number
  debounce?: number
}

export interface TreeAppOptions {
  id?: string
  tree: Node
  client?: Record<string, ClientDecl>
  sources?: Record<string, RowsDef>
  /** Events the app handles (return nothing); every event is logged in `events` first. */
  onEvent?(app: TreeApp, view: FakeView, event: ViewEventParams): void | Promise<void>
}

export class TreeApp implements FakeApp {
  readonly id: string
  readonly clientDecls: Record<string, ClientDecl>
  readonly state: FakeApp['state'] = {}
  readonly sources: Record<string, SourceSpec> = {}
  /** Every event the client sent, in order. */
  readonly events: ViewEventParams[] = []
  private readonly data: Record<string, Row[]> = {}
  private readonly seed: Record<string, Row[]> = {}

  constructor(private readonly o: TreeAppOptions) {
    this.id = o.id ?? 'demo'
    this.clientDecls = o.client ?? {}
    for (const [id, def] of Object.entries(o.sources ?? {})) {
      this.data[id] = def.rows
      this.seed[id] = def.rows.map((r) => ({ ...r }))
      const key = def.key ?? 'id'
      this.sources[id] = {
        decl: {
          kind: 'rows',
          key,
          page: def.page ?? 60,
          debounce: def.debounce ?? 150,
          params: {}
        },
        query: () => this.data[id],
        find: (k) => this.data[id].find((r) => String(r[key]) === String(k)) ?? null
      }
    }
  }

  tree(): Node {
    return this.o.tree
  }

  /** The live rows of a source: mutate them, then `host.refreshSources()`. */
  rows(id: string): Row[] {
    return this.data[id]
  }

  /** Put `key` in front of `beforeKey` (null = last), as an app storing a fractional rank would. */
  move(id: string, key: RowKey, beforeKey: RowKey | null, field = 'id'): void {
    const list = this.data[id]
    const from = list.findIndex((r) => String(r[field]) === String(key))
    if (from < 0) return
    const [row] = list.splice(from, 1)
    const at =
      beforeKey === null
        ? list.length
        : list.findIndex((r) => String(r[field]) === String(beforeKey))
    list.splice(at < 0 ? list.length : at, 0, row)
  }

  remove(id: string, key: RowKey, field = 'id'): void {
    this.data[id] = this.data[id].filter((r) => String(r[field]) !== String(key))
  }

  /** Events of one kind, newest last. */
  eventsOf(event: string): ViewEventParams[] {
    return this.events.filter((e) => e.event === event)
  }

  onEvent(view: FakeView, event: ViewEventParams): void | Promise<void> {
    this.events.push(event)
    return this.o.onEvent?.(this, view, event)
  }

  reset(): void {
    for (const [id, rows] of Object.entries(this.seed)) this.data[id] = rows.map((r) => ({ ...r }))
    this.events.length = 0
  }
}

/** `n` generated rows `{id, title, subtitle}` for a long list. */
export function generatedRows(n: number, label = 'Item'): Row[] {
  return Array.from({ length: n }, (_, i) => ({
    id: i + 1,
    title: `${label} ${i + 1}`,
    subtitle: `Row ${i + 1} of ${n}`
  }))
}

export type { Json }
