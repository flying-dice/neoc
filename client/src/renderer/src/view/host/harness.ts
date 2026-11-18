/**
 * Wires the pieces for tests and stories: the demo app, the fake host, the in-memory link and a real `Session`
 * on top of it. `openNotes()` opens one window the way the shell will (`view.open`).
 */
import { memoryStorage, type KeyValueStorage } from '../core/clientState'
import { Session } from '../core/session'
import type { StoreFactory } from '../core/store'
import { plainStore } from '../core/store'
import type { Timers } from '../core/timers'
import type { ViewCore, ViewEffects } from '../core/view'
import type { ThemeSet } from '../protocol'
import { FakeHost } from './fakeHost'
import { NotesApp, type NotesOptions } from './notesApp'
import { createMemoryLink, type MemoryLink, type WireMessage } from './wire'

export interface HarnessOptions {
  notes?: NotesOptions
  latencyMs?: number
  timers?: Timers
  createStore?: StoreFactory
  storage?: KeyValueStorage
  onMessage?: (m: WireMessage) => void
  onTheme?: (t: ThemeSet) => void
  onError?: (e: unknown) => void
}

export interface Harness {
  app: NotesApp
  host: FakeHost
  link: MemoryLink
  session: Session
  storage: KeyValueStorage
  /** Open the Notes window (waits for the connection). */
  openNotes(opts?: {
    windowId?: string
    size?: { w: number; h: number }
    effects?: ViewEffects
  }): Promise<ViewCore>
}

export function createHarness(options: HarnessOptions = {}): Harness {
  const app = new NotesApp(options.notes)
  const host = new FakeHost(app)
  const link = createMemoryLink({
    latencyMs: options.latencyMs,
    timers: options.timers,
    onMessage: options.onMessage
  })
  link.onConnection((c) => host.accept(c))
  const storage = options.storage ?? memoryStorage()
  const session = new Session({
    transport: link.transport,
    locale: 'en-GB',
    timeZone: 'UTC',
    createStore: options.createStore ?? plainStore,
    storage,
    timers: options.timers,
    onTheme: options.onTheme,
    onError: options.onError
  })
  session.start()
  return {
    app,
    host,
    link,
    session,
    storage,
    openNotes: ({ windowId = 'w1', size = { w: 480, h: 720 }, effects } = {}) =>
      session.openView({ app: 'notes', window: 'main', windowId, size }, { effects })
  }
}
