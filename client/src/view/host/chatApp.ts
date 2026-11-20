/**
 * "Chat-lite": the demo window for the ChatTranscript stories and tests. One `Screen` holding a `ChatTranscript`
 * bound to a `thread` source; the chat itself talks to a `FakeAgui` over `fetch` (it never reaches the app). The app
 * only sees the `send` and `stop` events, like the real Agents app does.
 */
import type { ClientDecl, Node, ViewEventParams } from '../protocol'
import type { FakeApp } from './fakeHost'

export interface ChatAppOptions {
  thread?: string
  agent?: string
  title?: string
  composer?: boolean
}

export class ChatApp implements FakeApp {
  readonly id = 'agents'
  readonly clientDecls: Record<string, ClientDecl> = {}
  readonly state: FakeApp['state'] = {}
  readonly sources: FakeApp['sources'] = {}
  readonly threads: NonNullable<FakeApp['threads']>
  /** The events the chat reported to the app, in order. */
  readonly events: { event: string; payload?: unknown }[] = []

  constructor(private readonly o: ChatAppOptions = {}) {
    this.threads = {
      chat: {
        kind: 'thread',
        params: { thread: o.thread ?? 'thread-1', agent: o.agent ?? 'claude' }
      }
    }
  }

  tree(): Node {
    return {
      type: 'Screen',
      key: 'main',
      props: { title: this.o.title ?? 'Chat', titleDisplay: 'inline' },
      children: [
        {
          type: 'ChatTranscript',
          key: 'chat',
          props: { thread: 'chat', composer: this.o.composer ?? true },
          on: { send: true, stop: true }
        }
      ]
    }
  }

  onEvent(_view: unknown, event: ViewEventParams): void {
    this.events.push({ event: event.event, payload: (event as { payload?: unknown }).payload })
  }
}
