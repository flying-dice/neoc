/**
 * Storybook support: a complete session on the in-memory wire with real timers and the reactive store, so a story
 * renders `ViewHost` against the fake host and its play function can drive the host side ("an agent appends",
 * "the network drops"). Story support only: the real client talks to the Rust host (wave B2).
 */
import type { ViewCore } from '../core/view'
import { reactiveStore } from '../store.svelte'
import type { FakeApp } from './fakeHost'
import { createAppHarness, createHarness, type Harness } from './harness'
import type { NotesApp, NotesOptions } from './notesApp'

export interface StoryScenario {
  notes?: NotesOptions
  latencyMs?: number
}

export interface StoryHandle<A extends FakeApp = NotesApp> {
  harness: Harness<A>
  view: ViewCore
  /** Stop reconnecting and drop the session (the story unmounted). */
  dispose(): void
}

export async function openStory(scenario: StoryScenario = {}): Promise<StoryHandle> {
  const harness = createHarness({
    ...scenario,
    createStore: reactiveStore,
    onError: () => undefined
  })
  const view = await harness.openNotes()
  return { harness, view, dispose: () => harness.session.stop() }
}

/** Wait until `check` holds (a play function waiting on a debounce or a round trip), or fail after `timeoutMs`. */
export async function until(check: () => boolean, label: string, timeoutMs = 4000): Promise<void> {
  const start = Date.now()
  while (!check()) {
    if (Date.now() - start > timeoutMs) throw new Error(`timed out waiting for ${label}`)
    await new Promise((r) => setTimeout(r, 20))
  }
}

/**
 * Hold a virtual list at the top until it has stayed there for twenty frames with its first rows loaded rather than
 * placeholders. Focus (or a scroll) can land while the scroll back to the top is still under way, and the host's rows
 * replace the placeholders a moment later: a snapshot or an assertion taken before that sees the list mid-flight.
 */
export async function settleAtTop(scroller: HTMLElement, root: ParentNode): Promise<void> {
  let still = 0
  for (let frame = 0; frame < 300 && still < 20; frame++) {
    await new Promise((r) => requestAnimationFrame(r))
    const placeholders = root.querySelectorAll('[aria-busy=true][role=listitem]').length
    if (scroller.scrollTop === 0 && placeholders === 0) still++
    else {
      still = 0
      scroller.scrollTop = 0
    }
  }
  if (still < 20) throw new Error('the list did not settle at the top')
}

/** Open a story window on any fake app (a `TreeApp`): the same session, real timers and the reactive store. */
export async function openAppStory<A extends FakeApp>(
  app: A,
  size?: { w: number; h: number }
): Promise<StoryHandle<A>> {
  const harness = createAppHarness(app, { createStore: reactiveStore, onError: () => undefined })
  const view = await harness.openNotes({ size })
  return { harness, view, dispose: () => harness.session.stop() }
}
