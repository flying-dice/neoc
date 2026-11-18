/**
 * A fake app that serves one fixed view tree with some state paths: what the kit stories (and tests) need to render
 * "JSON trees through the registry". Events from the client are recorded (`events`) and can be answered by a
 * handler that drives the host side: set state, push a toast. It is not a Luau runtime; the tree never changes
 * unless the handler sends ops.
 */
import type { ClientDecl, Json, Node, ViewEventParams } from '../protocol'
import type { FakeApp, FakeHost, FakeView, MergeMode } from './fakeHost'

export interface KitAppOptions {
  tree: Node
  /** State paths with their value and merge rule (default `text` for strings, `scalar` otherwise). */
  state?: Record<string, { value: Json; merge?: MergeMode }>
  client?: Record<string, ClientDecl>
  /** Answer an event (set state, toast, ...). Recorded in `events` first. */
  onEvent?: (view: FakeView, event: ViewEventParams, host: FakeHost) => void | Promise<void>
}

export class KitApp implements FakeApp {
  readonly id = 'kit'
  readonly clientDecls: Record<string, ClientDecl>
  readonly state: FakeApp['state']
  readonly sources: FakeApp['sources'] = {}
  /** Every event the client sent, in order. */
  readonly events: ViewEventParams[] = []
  host: FakeHost | null = null

  constructor(private readonly o: KitAppOptions) {
    this.clientDecls = o.client ?? {}
    this.state = Object.fromEntries(
      Object.entries(o.state ?? {}).map(([path, { value, merge }]) => [
        path,
        { value, merge: merge ?? (typeof value === 'string' ? 'text' : 'scalar') }
      ])
    )
  }

  tree(): Node {
    return structuredClone(this.o.tree)
  }

  async onEvent(view: FakeView, event: ViewEventParams): Promise<void> {
    this.events.push(event)
    if (this.host) await this.o.onEvent?.(view, event, this.host)
  }
}
