/**
 * Storybook support for the kit stories: a complete session on the in-memory wire (real timers, the reactive store)
 * serving one `KitApp` tree, so `ViewHost` renders JSON through the registry and a play function can drive the host
 * side ("an agent appends"). Story support only; the real client talks to the Rust host.
 */
import { memoryStorage } from '../core/clientState'
import { Session } from '../core/session'
import type { ViewCore } from '../core/view'
import { reactiveStore } from '../store.svelte'
import { FakeHost } from './fakeHost'
import { KitApp, type KitAppOptions } from './kitApp'
import { createMemoryLink } from './wire'

export interface KitStoryHandle {
  app: KitApp
  host: FakeHost
  view: ViewCore
  /** Links the window was asked to open (the shell would confirm and open them). */
  links: string[]
  /** Stop reconnecting and drop the session (the story unmounted). */
  dispose(): void
}

export async function openKitStory(options: KitAppOptions): Promise<KitStoryHandle> {
  const app = new KitApp(options)
  const host = new FakeHost(app)
  app.host = host
  const link = createMemoryLink({})
  link.onConnection((c) => host.accept(c))
  const session = new Session({
    transport: link.transport,
    locale: 'en-GB',
    timeZone: 'UTC',
    createStore: reactiveStore,
    storage: memoryStorage(),
    onError: () => undefined
  })
  session.start()
  const view = await session.openView({
    app: 'kit',
    window: 'main',
    windowId: 'w1',
    size: { w: 480, h: 720 }
  })
  const links: string[] = []
  return { app, host, view, links, dispose: () => session.stop() }
}
